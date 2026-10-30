//! The boot check for a newer horch release, and `horch self-update`
//! (design C2 and C3 of `ai_docs/specs/self-update/design.md`).
//!
//! A release is a GitHub release `vX.Y.Z` of `MattMakes/multi-herdr`. Its
//! assets are 1 flat archive per target plus `SHA256SUMS`. `self-update`
//! downloads the archive for [`TARGET`], verifies it, and runs the new
//! `horch install --dir <dir of this horch>` from a temp dir.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use horch_core::clock;
use horch_core::runtime::RuntimeContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The installed version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The target triple this horch was built for (set by `build.rs`).
pub const TARGET: &str = env!("HORCH_TARGET");

/// The GitHub API root of the repo that publishes the releases.
const API_BASE: &str = "https://api.github.com/repos/MattMakes/multi-herdr";

/// The boot check gives up after this long, so it never delays a fleet.
const BOOT_TIMEOUT: Duration = Duration::from_secs(2);

/// The time limit of `self-update`'s lookup and each download.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// A release archive is a few tens of MB. Stay bounded.
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// How long a cached lookup answers the boot check.
const CACHE_TTL_HOURS: i64 = 24;

/// The checksum asset of every release.
const SUMS_NAME: &str = "SHA256SUMS";

/// The fields of a GitHub release that horch reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub html_url: String,
    /// `(name, browser_download_url)` of each asset.
    pub assets: Vec<(String, String)>,
}

impl Release {
    fn asset_url(&self, name: &str) -> Option<&str> {
        self.assets
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, url)| url.as_str())
    }
}

/// `<state root>/self-check.json`: the last lookup of the boot check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    /// RFC 3339.
    pub checked_at: String,
    pub latest_tag: String,
    pub html_url: String,
}

/// `X.Y.Z` with an optional leading `v`. Anything else, a pre-release
/// suffix included, is `None`.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    });
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Whether `latest_tag` is a greater version than `current`. A version that
/// does not parse is never newer.
pub fn is_newer(latest_tag: &str, current: &str) -> bool {
    match (parse_version(latest_tag), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The body of `GET /releases/latest`.
pub fn parse_release(json: &str) -> Result<Release> {
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(Deserialize)]
    struct Raw {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        assets: Vec<Asset>,
    }
    let raw: Raw = serde_json::from_str(json).context("the release lookup is not a release")?;
    Ok(Release {
        tag: raw.tag_name,
        html_url: raw.html_url,
        assets: raw
            .assets
            .into_iter()
            .map(|a| (a.name, a.browser_download_url))
            .collect(),
    })
}

/// `multi-herdr-v<version>-<target>.tar.gz`, or `.zip` for a windows target.
pub fn archive_name(version_tag: &str, target: &str) -> String {
    let version = version_tag.strip_prefix('v').unwrap_or(version_tag);
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("multi-herdr-v{version}-{target}.{ext}")
}

/// The hex digest `SHA256SUMS` gives for `name`. Lines are
/// `<64 hex>  <name>`; a `*` before the name (binary mode) is allowed.
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let hex = words.next()?;
        let file = words.next()?.trim_start_matches('*');
        let valid = hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit());
        (valid && file == name && words.next().is_none()).then(|| hex.to_ascii_lowercase())
    })
}

/// Whether a lookup made at `checked_at` still answers at `now`.
pub fn is_fresh(now: DateTime<Utc>, checked_at: &str) -> bool {
    let Some(at) = clock::parse(checked_at) else {
        return false;
    };
    let age = now - at;
    age >= chrono::Duration::zero() && age < chrono::Duration::hours(CACHE_TTL_HOURS)
}

/// Whether `exe` sits in a cargo target layout: `target/{debug,release}/` or
/// `target/<triple>/{debug,release}/`. A dir named `target` alone is not one.
fn is_cargo_build(exe: &Path) -> bool {
    let name_of = |dir: Option<&Path>| dir.and_then(Path::file_name).map(|n| n == "target");
    let profile = exe.parent();
    let in_profile_dir = profile
        .and_then(Path::file_name)
        .is_some_and(|n| n == "debug" || n == "release");
    in_profile_dir
        && (name_of(profile.and_then(Path::parent)) == Some(true)
            || name_of(profile.and_then(Path::parent).and_then(Path::parent)) == Some(true))
}

/// The directory `self-update` installs into: the canonical parent of the
/// running horch, else `default_dir`. A horch in a cargo `target` layout is a
/// source build, and a release must not overwrite it.
pub fn install_dir(current_exe: Option<&Path>, default_dir: &Path) -> Result<PathBuf> {
    let Some(exe) = current_exe else {
        return Ok(default_dir.to_path_buf());
    };
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    if is_cargo_build(&exe) {
        bail!(
            "{} is a source build; run `git pull` and `just install` to update it",
            exe.display()
        );
    }
    exe.parent()
        .map(Path::to_path_buf)
        .with_context(|| format!("{} has no parent directory", exe.display()))
}

/// The binaries every release archive holds, which are absent from `dir`.
/// An install with only some of them would mix versions.
fn missing_binaries(dir: &Path) -> Vec<String> {
    [
        "horch",
        super::install::DATASET_BIN,
        super::install::HERDR_INSTALL_BIN,
    ]
    .iter()
    .map(|bin| super::install::exe_name(bin))
    .filter(|file| !dir.join(file).is_file())
    .collect()
}

/// The 2 lines the boot check prints.
fn notice_text(tag: &str, html_url: &str) -> String {
    format!(
        "horch: NOTE: multi-herdr {tag} is out (you have v{CURRENT}): {html_url}\n\
         horch: NOTE: run `horch self-update` to install it."
    )
}

fn cache_path(state_root: &Path) -> PathBuf {
    state_root.join("self-check.json")
}

/// The boot check's decision: a fresh cache answers, else `fetch` does and
/// its answer is cached. Returns the notice to print, if any. Every error is
/// silent: `None`.
fn boot_notice(
    state_root: &Path,
    now: DateTime<Utc>,
    current: &str,
    fetch: impl FnOnce() -> Result<Option<Release>>,
) -> Option<String> {
    let path = cache_path(state_root);
    let cached = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
        .filter(|cache| is_fresh(now, &cache.checked_at));
    let cache = match cached {
        Some(cache) => cache,
        None => {
            let release = fetch().ok()??;
            let cache = Cache {
                checked_at: clock::stamp(now),
                latest_tag: release.tag,
                html_url: release.html_url,
            };
            if let Ok(text) = serde_json::to_string(&cache) {
                let _ = std::fs::create_dir_all(state_root);
                let _ = std::fs::write(&path, text);
            }
            cache
        }
    };
    is_newer(&cache.latest_tag, current).then(|| notice_text(&cache.latest_tag, &cache.html_url))
}

/// Warn on hot paths using only local state, independent of agent update policy.
/// Both reasons share one line, even when the cache is arbitrarily old.
pub fn stale_warning(ctx: &RuntimeContext) {
    if let Some(line) = stale_warning_text(ctx) {
        eprintln!("{line}");
    }
}

/// Doctor preserves the boot check's bounded cache refresh, but coalesces its
/// result with installed-binary evidence into the single offline warning.
pub fn doctor_warning(ctx: &RuntimeContext) {
    if let Some(line) = doctor_warning_text(ctx, std::io::stderr().is_terminal(), || {
        fetch_latest(BOOT_TIMEOUT)
    }) {
        eprintln!("{line}");
    }
}

fn doctor_warning_text(
    ctx: &RuntimeContext,
    terminal: bool,
    fetch: impl FnOnce() -> Result<Option<Release>>,
) -> Option<String> {
    if terminal && !ctx.settings.no_update && !ctx.settings.no_self_check {
        // Preserve refresh/cache side effects without printing the old two-line notice.
        let _ = boot_notice(&ctx.paths.state_root, clock::now(), CURRENT, fetch);
    }
    stale_warning_text(ctx)
}

fn stale_warning_text(ctx: &RuntimeContext) -> Option<String> {
    if ctx.settings.no_self_check {
        return None;
    }
    let mut reasons = Vec::new();
    if let Some(cache) = std::fs::read_to_string(cache_path(&ctx.paths.state_root))
        .ok()
        .and_then(|s| serde_json::from_str::<Cache>(&s).ok())
        .filter(|c| is_newer(&c.latest_tag, CURRENT))
    {
        reasons.push(format!(
            "running v{CURRENT}; cached latest {}",
            cache.latest_tag
        ));
    }
    if let (Some(running), Some(installed)) = (&ctx.bins.current_exe, &ctx.bins.installed_horch) {
        // Distinct paths are compared by content to ignore identical copies.
        let same_path = running
            .canonicalize()
            .ok()
            .zip(installed.canonicalize().ok())
            .is_some_and(|(a, b)| a == b);
        if !same_path
            && std::fs::read(running)
                .ok()
                .zip(std::fs::read(installed).ok())
                .is_some_and(|(a, b)| a != b)
        {
            reasons.push(format!(
                "running {} differs from installed {}",
                running.display(),
                installed.display()
            ));
        }
    }
    (!reasons.is_empty()).then(|| format!("horch: WARNING: stale horch: {}", reasons.join("; ")))
}

/// Set on the horch that [`update_before_fleet`] restarts into, so the new
/// process does not update again.
const UPDATED_ENV: &str = "HORCH_SELF_UPDATED";

/// The update step of `horch fleet` and `horch orchestration`, before
/// anything else runs: when a newer release is out, install it over this
/// horch and restart the same command on it. Skipped like the boot notice
/// (`--no-update`, `HORCH_NO_UPDATE=1`, `HORCH_NO_SELF_CHECK=1`, no
/// terminal) and in the process it restarted. A source build gets the
/// notice instead. A failed update never stops the fleet: it starts on this
/// version.
pub fn update_before_fleet(ctx: &RuntimeContext, no_update: bool) {
    if skip_boot_update(ctx, no_update) {
        return;
    }
    let fetch = || fetch_latest(BOOT_TIMEOUT);
    if let Some(notice) = boot_notice(&ctx.paths.state_root, clock::now(), CURRENT, fetch) {
        install_and_restart(ctx, &notice);
    }
}

/// Whether the boot update step is off: a flag, a setting, the restarted
/// process, or no terminal.
fn skip_boot_update(ctx: &RuntimeContext, no_update: bool) -> bool {
    no_update
        || ctx.settings.no_update
        || ctx.settings.no_self_check
        || std::env::var_os(UPDATED_ENV).is_some()
        || !std::io::stderr().is_terminal()
}

/// Install the newer release over this horch and restart on it. A source
/// build prints `notice` instead.
fn install_and_restart(ctx: &RuntimeContext, notice: &str) {
    let Ok(dir) = target_dir(ctx) else {
        // A source build: `git pull` and `just install` update it.
        eprintln!("{notice}");
        return;
    };
    eprintln!(
        "horch: a newer multi-herdr is out; installing it before the fleet starts \
         (--no-update skips this)..."
    );
    let installed = fetch_latest(DOWNLOAD_TIMEOUT)
        .and_then(|release| release.context("the release went away"))
        .and_then(|release| install_release(ctx, &release, &dir).map(|()| release));
    match installed {
        Ok(release) => {
            eprintln!("horch: updated v{CURRENT} -> {}; restarting", release.tag);
            let error = restart(&dir);
            eprintln!(
                "horch: NOTE: cannot start the new horch ({error}); continuing on v{CURRENT}"
            );
        }
        Err(error) => {
            eprintln!("horch: NOTE: the update failed ({error:#}); continuing on v{CURRENT}");
        }
    }
}

/// Run `<dir>/horch` with this process's arguments and [`UPDATED_ENV`]. On
/// unix it replaces this process; elsewhere it waits and exits with the
/// child's code. Returns only the error of a start that failed.
fn restart(dir: &Path) -> std::io::Error {
    use std::io::Write;
    let exe = dir.join(format!("horch{}", std::env::consts::EXE_SUFFIX));
    let mut cmd = Command::new(&exe);
    cmd.args(std::env::args_os().skip(1)).env(UPDATED_ENV, "1");
    let _ = std::io::stdout().flush();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => error,
        }
    }
}

/// The directory a release installs into. `HORCH_SELF_UPDATE_DIR` is for
/// tests only: the test binary lives under `target/`, which `install_dir`
/// refuses.
fn target_dir(ctx: &RuntimeContext) -> Result<PathBuf> {
    match std::env::var_os("HORCH_SELF_UPDATE_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => install_dir(
            ctx.bins.current_exe.as_deref(),
            &super::install::default_dir(ctx),
        ),
    }
}

/// `horch self-update [--check]`.
pub fn self_update(ctx: &RuntimeContext, check: bool) -> Result<ExitCode> {
    match fetch_latest(DOWNLOAD_TIMEOUT)? {
        None => println!("horch: no multi-herdr release is published yet (you have v{CURRENT})"),
        Some(release) if !is_newer(&release.tag, CURRENT) => {
            println!("horch is up to date (v{CURRENT})")
        }
        Some(release) if check => println!(
            "multi-herdr {} is out (you have v{CURRENT}): {}",
            release.tag, release.html_url
        ),
        Some(release) => install_latest(ctx, &release)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn install_latest(ctx: &RuntimeContext, release: &Release) -> Result<()> {
    install_release(ctx, release, &target_dir(ctx)?)?;
    println!("updated horch v{CURRENT} -> {}", release.tag);
    Ok(())
}

/// Download `release` for this target, check its digest and its binaries,
/// and run its `horch install --dir <dir>`. Clears the boot check's cache.
fn install_release(ctx: &RuntimeContext, release: &Release, dir: &Path) -> Result<()> {
    let name = archive_name(&release.tag, TARGET);
    let archive = verified_archive(release, &name)?;
    let tmp = tempfile::tempdir().context("cannot make a temp dir")?;
    extract(&archive, tmp.path())?;
    let absent = missing_binaries(tmp.path());
    if !absent.is_empty() {
        bail!(
            "{name} of release {} lacks {}; nothing is installed",
            release.tag,
            absent.join(", ")
        );
    }
    let new_horch = tmp
        .path()
        .join(format!("horch{}", std::env::consts::EXE_SUFFIX));
    run_install(&new_horch, dir)?;
    let _ = std::fs::remove_file(cache_path(&ctx.paths.state_root));
    Ok(())
}

/// Download the archive `name` of `release` and check it against the
/// release's SHA256SUMS.
fn verified_archive(release: &Release, name: &str) -> Result<Vec<u8>> {
    let missing = |what: &str| {
        anyhow::anyhow!(
            "release {} has no {what} for target {TARGET}: {}",
            release.tag,
            release.html_url
        )
    };
    let archive_url = release
        .asset_url(name)
        .ok_or_else(|| missing(&format!("archive {name}")))?;
    let sums_url = release
        .asset_url(SUMS_NAME)
        .ok_or_else(|| missing(SUMS_NAME))?;
    let expected = expected_digest(release, name, sums_url)?;
    let archive = download(archive_url)?;
    verify_digest(&archive, &expected)?;
    Ok(archive)
}

/// The digest that the release's SHA256SUMS gives for `name`.
fn expected_digest(release: &Release, name: &str, sums_url: &str) -> Result<String> {
    let sums = String::from_utf8(download(sums_url)?).context("SHA256SUMS is not text")?;
    parse_sums(&sums, name)
        .with_context(|| format!("SHA256SUMS of {} has no line for {name}", release.tag))
}

/// Run `<new_horch> install --dir <dir>`.
fn run_install(new_horch: &Path, dir: &Path) -> Result<()> {
    let status = Command::new(new_horch)
        .arg("install")
        .arg("--dir")
        .arg(dir)
        .status()
        .with_context(|| format!("cannot run {}", new_horch.display()))?;
    if !status.success() {
        bail!(
            "the new horch install exited with {}",
            status.code().unwrap_or(1)
        );
    }
    Ok(())
}

// The network code below is the twin of `crates/herdr-install/src/net.rs`.

/// `HORCH_RELEASE_API` replaces [`API_BASE`]. It is for tests only, which
/// point it at a local server.
fn api_base() -> String {
    std::env::var("HORCH_RELEASE_API")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| API_BASE.to_string())
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// GitHub rejects a request without a User-Agent.
fn user_agent() -> String {
    format!("horch/{CURRENT}")
}

/// `GET /releases/latest`. `None` when the repo has no release yet (404).
fn fetch_latest(timeout: Duration) -> Result<Option<Release>> {
    let url = format!("{}/releases/latest", api_base());
    let response = agent(timeout)
        .get(&url)
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call();
    match response {
        Ok(mut response) => read_release(&mut response, &url).map(Some),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot reach {url}")),
    }
}

fn read_release(response: &mut ureq::http::Response<ureq::Body>, url: &str) -> Result<Release> {
    let body = response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))?;
    parse_release(&body)
}

/// Download 1 release asset into memory.
fn download(url: &str) -> Result<Vec<u8>> {
    let bytes = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .header("User-Agent", &user_agent())
        .call()
        .with_context(|| format!("download failed from {url}"))?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .with_context(|| format!("reading the download from {url}"))?;
    if bytes.is_empty() {
        bail!("download from {url} was empty");
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verify_digest(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        bail!(
            "checksum mismatch: SHA256SUMS says {expected}, the download hashes to {actual}. \
             Nothing is installed."
        );
    }
    Ok(())
}

/// A release archive is flat: an entry name with a separator or `..` is
/// refused.
fn flat_name(name: &str) -> Result<&str> {
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        bail!("the release archive holds an unexpected entry {name:?}");
    }
    Ok(name)
}

#[cfg(unix)]
fn extract(archive: &[u8], dir: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().context("the archive is not a .tar.gz")? {
        extract_entry(entry, dir)?;
    }
    Ok(())
}

#[cfg(unix)]
fn extract_entry(
    entry: std::io::Result<tar::Entry<'_, impl std::io::Read>>,
    dir: &Path,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut entry = entry.context("reading the archive")?;
    let raw = entry.path().context("reading the archive")?;
    let name = flat_name(&raw.to_string_lossy())?.to_string();
    if !entry.header().entry_type().is_file() {
        bail!("the release archive entry {name:?} is not a file");
    }
    let dest = dir.join(&name);
    let mut file =
        std::fs::File::create(&dest).with_context(|| format!("cannot write {}", dest.display()))?;
    std::io::copy(&mut entry, &mut file).context("reading the archive")?;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(windows)]
fn extract(archive: &[u8], dir: &Path) -> Result<()> {
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(archive)).context("the archive is not a .zip")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("reading the archive")?;
        let name = flat_name(entry.name())?.to_string();
        if !entry.is_file() {
            bail!("the release archive entry {name:?} is not a file");
        }
        let dest = dir.join(&name);
        let mut file = std::fs::File::create(&dest)
            .with_context(|| format!("cannot write {}", dest.display()))?;
        std::io::copy(&mut entry, &mut file).context("reading the archive")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn g49_doctor_refreshes_expired_cache_and_coalesces_one_warning() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = horch_core::runtime::RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new(dir.path())
                .with("HORCH_STATE_DIR", &dir.path().to_string_lossy()),
        )
        .unwrap();
        let old = Cache {
            checked_at: "2020-01-01T00:00:00Z".into(),
            latest_tag: "v98.0.0".into(),
            html_url: "https://example.invalid/old".into(),
        };
        std::fs::write(cache_path(dir.path()), serde_json::to_vec(&old).unwrap()).unwrap();
        let line = doctor_warning_text(&ctx, true, || {
            Ok(Some(Release {
                tag: "v99.0.0".into(),
                html_url: "https://example.invalid/new".into(),
                assets: vec![],
            }))
        })
        .unwrap();
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("v99.0.0"));
        let fresh: Cache =
            serde_json::from_slice(&std::fs::read(cache_path(dir.path())).unwrap()).unwrap();
        assert!(is_fresh(clock::now(), &fresh.checked_at));
        assert_eq!(fresh.latest_tag, "v99.0.0");
        assert!(
            doctor_warning_text(&ctx, true, || panic!("fresh 24h cache must not fetch")).is_some()
        );
        let mut disabled = ctx.clone();
        disabled.settings.no_update = true;
        assert!(
            doctor_warning_text(&disabled, true, || panic!("no-update must not fetch")).is_some()
        );
        assert!(
            doctor_warning_text(&ctx, false, || panic!("piped doctor stays offline")).is_some()
        );
        disabled.settings.no_self_check = true;
        assert!(doctor_warning_text(&disabled, true, || panic!("pin must not fetch")).is_none());
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn target_is_baked_in_by_build_rs() {
        assert_eq!(TARGET, "aarch64-apple-darwin");
    }

    #[test]
    fn parse_version_takes_x_y_z_with_an_optional_v() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        for bad in [
            "",
            "v",
            "0.2",
            "0.2.0.1",
            "v0.2.0-rc1",
            "0.2.x",
            "+1.0.0",
            "1..0",
            "vv1.0.0",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn is_newer_compares_numerically() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("v0.3.0-rc1", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
    }

    #[test]
    fn parse_release_reads_the_tag_url_and_assets() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://x/r","name":"ignored",
            "assets":[{"name":"SHA256SUMS","browser_download_url":"https://x/s","size":1}]}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(
            release,
            Release {
                tag: "v0.2.0".into(),
                html_url: "https://x/r".into(),
                assets: vec![("SHA256SUMS".into(), "https://x/s".into())],
            }
        );
        assert_eq!(release.asset_url("SHA256SUMS"), Some("https://x/s"));
        assert!(parse_release(r#"{"message":"Not Found"}"#).is_err());
    }

    #[test]
    fn archive_name_follows_the_release_contract() {
        assert_eq!(
            archive_name("v0.2.0", "aarch64-apple-darwin"),
            "multi-herdr-v0.2.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            archive_name("0.2.0", "x86_64-pc-windows-msvc"),
            "multi-herdr-v0.2.0-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn parse_sums_finds_the_line_of_1_archive() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text = format!("{a}  multi-herdr-v0.2.0-x.tar.gz\n{b} *multi-herdr-v0.2.0-y.zip\n");
        assert_eq!(parse_sums(&text, "multi-herdr-v0.2.0-x.tar.gz"), Some(a));
        assert_eq!(
            parse_sums(&text, "multi-herdr-v0.2.0-y.zip"),
            Some("b".repeat(64))
        );
        assert_eq!(parse_sums(&text, "multi-herdr-v0.2.0-z.tar.gz"), None);
        assert_eq!(parse_sums("abc  short.tar.gz", "short.tar.gz"), None);
    }

    #[test]
    fn is_fresh_is_24_hours() {
        let now = clock::parse("2026-10-20T12:00:00Z").unwrap();
        assert!(is_fresh(now, "2026-10-20T12:00:00Z"));
        assert!(is_fresh(now, "2026-10-19T12:00:01Z"));
        assert!(!is_fresh(now, "2026-10-19T12:00:00Z"));
        assert!(!is_fresh(now, "2026-10-21T12:00:00Z"), "a future stamp");
        assert!(!is_fresh(now, "yesterday"));
    }

    #[test]
    fn install_dir_is_the_exe_parent_and_refuses_a_cargo_build() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fallback = Path::new("/fallback");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/horch"), "").unwrap();
        assert_eq!(
            install_dir(Some(&root.join("bin/horch")), fallback).unwrap(),
            root.join("bin")
        );
        assert_eq!(install_dir(None, fallback).unwrap(), fallback);

        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/horch"), "").unwrap();
        let err = install_dir(Some(&root.join("target/debug/horch")), fallback)
            .unwrap_err()
            .to_string();
        assert!(err.contains("source build"), "{err}");
        assert!(err.contains("git pull"), "{err}");
        assert!(err.contains("just install"), "{err}");
    }

    #[test]
    fn install_dir_refuses_only_a_cargo_target_layout() {
        let fallback = Path::new("/fallback");
        // Nothing here exists, so canonicalize keeps the path as written.
        assert_eq!(
            install_dir(Some(Path::new("/opt/target/bin/horch")), fallback).unwrap(),
            Path::new("/opt/target/bin")
        );
        for exe in [
            "/x/target/release/horch",
            "/x/target/debug/horch",
            "/x/target/aarch64-apple-darwin/release/horch",
        ] {
            let err = install_dir(Some(Path::new(exe)), fallback)
                .unwrap_err()
                .to_string();
            assert!(err.contains("source build"), "{exe}: {err}");
        }
        // `release` alone, or `target` 3 levels up, is not a cargo layout.
        assert!(install_dir(Some(Path::new("/x/release/horch")), fallback).is_ok());
        assert!(install_dir(Some(Path::new("/x/target/a/b/release/horch")), fallback).is_ok());
    }

    #[test]
    fn missing_binaries_names_each_absent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = super::super::install::exe_name;
        assert_eq!(
            missing_binaries(tmp.path()),
            vec![
                exe("horch"),
                exe("multi-herdr-dataset"),
                exe("herdr-install")
            ]
        );
        std::fs::write(tmp.path().join(exe("horch")), "").unwrap();
        std::fs::write(tmp.path().join(exe("herdr-install")), "").unwrap();
        assert_eq!(
            missing_binaries(tmp.path()),
            vec![exe("multi-herdr-dataset")]
        );
        std::fs::write(tmp.path().join(exe("multi-herdr-dataset")), "").unwrap();
        assert!(missing_binaries(tmp.path()).is_empty());
    }

    #[test]
    fn flat_name_refuses_paths() {
        assert!(flat_name("horch").is_ok());
        for bad in ["", "a/b", "a\\b", "..", "../horch", "/horch"] {
            assert!(flat_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn verify_digest_refuses_a_mismatch() {
        let digest = sha256_hex(b"abc");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(verify_digest(b"abc", &digest.to_uppercase()).is_ok());
        assert!(verify_digest(b"abd", &digest).is_err());
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.into(),
            html_url: format!("https://x/{tag}"),
            assets: vec![],
        }
    }

    fn write_cache(state: &Path, checked_at: &str, tag: &str) {
        let cache = Cache {
            checked_at: checked_at.into(),
            latest_tag: tag.into(),
            html_url: format!("https://x/{tag}"),
        };
        std::fs::write(cache_path(state), serde_json::to_string(&cache).unwrap()).unwrap();
    }

    #[test]
    fn boot_notice_uses_a_fresh_cache_without_a_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let now = clock::parse("2026-10-20T12:00:00Z").unwrap();
        write_cache(tmp.path(), "2026-10-20T08:00:00Z", "v0.3.0");
        let fetched = Cell::new(0);
        let text = boot_notice(tmp.path(), now, "0.2.0", || {
            fetched.set(fetched.get() + 1);
            Ok(Some(release("v9.0.0")))
        });
        assert_eq!(fetched.get(), 0);
        let text = text.unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("horch: NOTE: multi-herdr v0.3.0 is out (you have v"));
        assert!(lines[0].ends_with("): https://x/v0.3.0"));
        assert_eq!(
            lines[1],
            "horch: NOTE: run `horch self-update` to install it."
        );
    }

    #[test]
    fn boot_notice_fetches_once_on_a_stale_cache_and_caches_the_answer() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let now = clock::parse("2026-10-20T12:00:00Z").unwrap();
        std::fs::create_dir_all(&state).unwrap();
        write_cache(&state, "2026-10-18T12:00:00Z", "v0.2.0");
        let fetched = Cell::new(0);
        let text = boot_notice(&state, now, "0.2.0", || {
            fetched.set(fetched.get() + 1);
            Ok(Some(release("v0.3.0")))
        });
        assert_eq!(fetched.get(), 1);
        assert!(text.unwrap().contains("v0.3.0 is out"));
        let cache: Cache =
            serde_json::from_str(&std::fs::read_to_string(cache_path(&state)).unwrap()).unwrap();
        assert_eq!(cache.checked_at, "2026-10-20T12:00:00Z");
        assert_eq!(cache.latest_tag, "v0.3.0");
    }

    #[test]
    fn boot_notice_is_silent_on_an_error_no_release_or_an_equal_version() {
        let tmp = tempfile::tempdir().unwrap();
        let now = clock::parse("2026-10-20T12:00:00Z").unwrap();
        let failing = || -> Result<Option<Release>> { bail!("offline") };
        assert_eq!(boot_notice(tmp.path(), now, "0.2.0", failing), None);
        assert!(!cache_path(tmp.path()).exists());
        assert_eq!(boot_notice(tmp.path(), now, "0.2.0", || Ok(None)), None);
        assert_eq!(
            boot_notice(tmp.path(), now, "0.2.0", || Ok(Some(release("v0.2.0")))),
            None
        );
        // A dev build ahead of the latest release is silent too.
        write_cache(tmp.path(), "2026-10-20T11:00:00Z", "v0.2.0");
        assert_eq!(
            boot_notice(tmp.path(), now, "0.3.0", || Ok(Some(release("v0.2.0")))),
            None
        );
    }
}
