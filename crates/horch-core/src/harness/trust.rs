//! Harness trust in a folder (PRE-14, ARC-29). Claude, Codex and
//! Antigravity show a trust dialog on their first launch in a folder; a
//! pane stopped at it does no work. horch reads the stores, and before an
//! agent starts it writes the one missing entry for the agent's workdir
//! ([`ensure_trusted`]): atomic, every other key kept, never `/`, the home
//! directory or an ancestor of it, and never over an operator's explicit
//! Codex `trust_level = "untrusted"`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context as _;

use serde::{Deserialize, Serialize};

use super::HarnessKind;

/// What a harness has recorded about the repository root. A harness that
/// has not trusted it stops a new candidate pane at a trust dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Trusted,
    Untrusted,
    /// horch cannot read the harness's decision.
    Unknown,
}

/// One harness's trust in the repository root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessTrust {
    /// [`HarnessKind::as_str`].
    pub harness: String,
    pub state: TrustState,
    /// Why, naming the store file. Never any content of the file: it can
    /// hold tokens.
    pub reason: String,
}

impl HarnessTrust {
    fn new(harness: HarnessKind, state: TrustState, reason: impl Into<String>) -> Self {
        HarnessTrust {
            harness: harness.as_str().to_string(),
            state,
            reason: reason.into(),
        }
    }
}

/// Whether `harness` shows a trust dialog on its first launch in a
/// repository. opencode, pi and prime have no trust step.
pub fn asks_for_trust(harness: HarnessKind) -> bool {
    matches!(
        harness,
        HarnessKind::Claude | HarnessKind::Codex | HarnessKind::Antigravity
    )
}

/// The store file names the reasons use. Claude keeps `.claude.json` in
/// `$CLAUDE_CONFIG_DIR` when it is set, else in the home directory (see
/// [`claude_config_file`]).
pub const CLAUDE_TRUST_FILE: &str = "$CLAUDE_CONFIG_DIR/.claude.json";
pub const CODEX_TRUST_FILE: &str = "$CODEX_HOME/config.toml";
pub const ANTIGRAVITY_TRUST_FILE: &str = "~/.gemini/antigravity-cli/settings.json";

/// The file Claude keeps its global config and trust decisions in:
/// `$CLAUDE_CONFIG_DIR/.claude.json` when the variable is set, else
/// `<home>/.claude.json`. Seen on claude 2.1.289: with the variable set, it
/// writes `.claude.json` in that directory and leaves the home one alone.
pub fn claude_config_file(home: &Path, claude_config_dir: Option<&Path>) -> PathBuf {
    claude_config_dir.unwrap_or(home).join(".claude.json")
}

/// Claude's trust in `roots` (the root and its canonical form): the key
/// `projects["<root>"].hasTrustDialogAccepted` is `true` in
/// [`claude_config_file`]. `file` is the file text, `None` when it does not exist.
/// A parent folder's entry does not count: Claude keys trust on the
/// repository root.
pub fn claude_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let claude = HarnessKind::Claude;
    let Some(text) = file else {
        return HarnessTrust::new(
            claude,
            TrustState::Untrusted,
            format!("{CLAUDE_TRUST_FILE} does not exist"),
        );
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return HarnessTrust::new(
            claude,
            TrustState::Unknown,
            format!("{CLAUDE_TRUST_FILE} is not valid JSON"),
        );
    };
    let accepted = roots.iter().any(|root| {
        doc.get("projects")
            .and_then(|p| p.get(root.to_string_lossy().as_ref()))
            .and_then(|e| e.get("hasTrustDialogAccepted"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    });
    if accepted {
        HarnessTrust::new(
            claude,
            TrustState::Trusted,
            format!("{CLAUDE_TRUST_FILE} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(
            claude,
            TrustState::Untrusted,
            format!("{CLAUDE_TRUST_FILE} has no accepted trust for the repository root"),
        )
    }
}

/// Codex's trust in `roots`: a table `[projects."<root>"]` with
/// `trust_level = "trusted"` in `$CODEX_HOME/config.toml`. `file` is the file
/// text, `None` when it does not exist.
pub fn codex_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let codex = HarnessKind::Codex;
    let Some(text) = file else {
        return HarnessTrust::new(
            codex,
            TrustState::Untrusted,
            format!("{CODEX_TRUST_FILE} does not exist"),
        );
    };
    let roots: Vec<String> = roots
        .iter()
        .map(|r| r.to_string_lossy().into_owned())
        .collect();
    let mut in_root = false;
    let mut trusted = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_root = toml_project_header(line).is_some_and(|p| roots.contains(&p));
        } else if in_root {
            if let Some(level) = toml_string_value(line, "trust_level") {
                trusted = level == "trusted";
            }
        }
    }
    if trusted {
        HarnessTrust::new(
            codex,
            TrustState::Trusted,
            format!("{CODEX_TRUST_FILE} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(
            codex,
            TrustState::Untrusted,
            format!("{CODEX_TRUST_FILE} has no trust_level = \"trusted\" for the repository root"),
        )
    }
}

/// Antigravity's trust in `roots`: the list `trustedWorkspaces` in
/// `~/.gemini/antigravity-cli/settings.json` holds the root. `file` is the
/// file text, `None` when it does not exist. Trust is exact-path: a parent
/// folder or a subdirectory does not count. Seen on agy 1.2.17
/// (`docs/live-checks/harnesses.md`): `{"trustedWorkspaces": ["<dir>", ...]}`.
pub fn antigravity_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let agy = HarnessKind::Antigravity;
    let Some(text) = file else {
        return HarnessTrust::new(
            agy,
            TrustState::Untrusted,
            format!("{ANTIGRAVITY_TRUST_FILE} does not exist"),
        );
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return HarnessTrust::new(
            agy,
            TrustState::Unknown,
            format!("{ANTIGRAVITY_TRUST_FILE} is not valid JSON"),
        );
    };
    let trusted = doc
        .get("trustedWorkspaces")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|dirs| {
            dirs.iter()
                .filter_map(serde_json::Value::as_str)
                .any(|dir| roots.iter().any(|root| Path::new(dir) == root))
        });
    if trusted {
        HarnessTrust::new(
            agy,
            TrustState::Trusted,
            format!("{ANTIGRAVITY_TRUST_FILE} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(
            agy,
            TrustState::Untrusted,
            format!(
                "{ANTIGRAVITY_TRUST_FILE} has no trustedWorkspaces entry for the repository root"
            ),
        )
    }
}

/// The path of a `[projects."<path>"]` (or `[projects.'<path>']`) header.
fn toml_project_header(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.trim_start();
    let rest = inner
        .strip_prefix("projects")?
        .trim_start()
        .strip_prefix('.')?;
    let rest = rest.trim_start();
    let (key, after) = toml_quoted(rest)?;
    (after.trim() == "]").then_some(key)
}

/// `"<value>"` or `'<value>'` for `key = ...`, `None` for any other line.
fn toml_string_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    toml_quoted(rest.trim_start()).map(|(value, _)| value)
}

/// A TOML basic (`"..."`, with `\\` and `\"` escapes) or literal (`'...'`)
/// string at the start of `text`, and the text after it.
fn toml_quoted(text: &str) -> Option<(String, &str)> {
    let mut chars = text.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if quote == '"' && escaped {
            out.push(c);
            escaped = false;
        } else if quote == '"' && c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((out, &text[i + c.len_utf8()..]));
        } else {
            out.push(c);
        }
    }
    None
}

/// The one-time command that records `harness`'s trust in `root`, for when
/// [`ensure_trusted`] skipped or failed.
pub fn trust_fix(harness: HarnessKind, root: &Path) -> String {
    let quoted = format!("'{}'", root.to_string_lossy().replace('\'', "'\\''"));
    match harness {
        HarnessKind::Claude => {
            format!("cd {quoted} && claude  (choose \"Yes, I trust this folder\", then exit)")
        }
        HarnessKind::Codex => {
            format!("cd {quoted} && codex  (choose \"Trust and continue\", then exit)")
        }
        HarnessKind::Antigravity => {
            format!("cd {quoted} && agy  (accept the trust question, then exit)")
        }
        other => format!(
            "cd {quoted} && {}  (accept the trust question, then exit)",
            other.as_str()
        ),
    }
}

/// What each harness in `harnesses` has recorded for `roots` (the
/// repository root and its canonical form). It reads the stores and keeps
/// only the verdict: the files can hold tokens, so nothing else of them
/// leaves this function. `home` is the home directory (agy keeps its store
/// under it); the two directories are `$CLAUDE_CONFIG_DIR` and
/// `$CODEX_HOME` when they are set.
pub fn read_trust(
    harnesses: &[HarnessKind],
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
    roots: &[PathBuf],
) -> Vec<HarnessTrust> {
    harnesses
        .iter()
        .map(|&harness| match harness {
            HarnessKind::Claude => match read_store(&claude_config_file(home, claude_config_dir)) {
                Ok(text) => claude_trust(text.as_deref(), roots),
                Err(()) => unreadable(harness, CLAUDE_TRUST_FILE),
            },
            HarnessKind::Codex => {
                let file = super::codex::codex_home(home, codex_home_dir).join("config.toml");
                match read_store(&file) {
                    Ok(text) => codex_trust(text.as_deref(), roots),
                    Err(()) => unreadable(harness, CODEX_TRUST_FILE),
                }
            }
            HarnessKind::Antigravity => {
                let file = home.join(".gemini/antigravity-cli/settings.json");
                match read_store(&file) {
                    Ok(text) => antigravity_trust(text.as_deref(), roots),
                    Err(()) => unreadable(harness, ANTIGRAVITY_TRUST_FILE),
                }
            }
            other => HarnessTrust::new(
                other,
                TrustState::Unknown,
                format!("horch cannot read where {} keeps its trust", other.as_str()),
            ),
        })
        .collect()
}

/// The text of a trust store, `None` when it does not exist, `Err` when it
/// cannot be read.
fn read_store(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn unreadable(harness: HarnessKind, file: &str) -> HarnessTrust {
    HarnessTrust::new(
        harness,
        TrustState::Unknown,
        format!("{file} cannot be read"),
    )
}

/// What [`ensure_trusted`] did for one harness and workdir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustWrite {
    /// The store already trusted the workdir. horch wrote nothing.
    AlreadyTrusted,
    /// horch wrote the trust entry to this store file.
    Written(PathBuf),
    /// horch did not write, for this reason. It names the store file or the
    /// path, never any content of the file.
    Skipped(String),
}

/// Make `harness` trust `workdir` before an agent starts in it (ARC-29).
///
/// It reads the store first and writes only when the store does not trust
/// the workdir or its canonical form, so most launches write nothing. The
/// write keeps every other key and value and is atomic: a temporary file in
/// the same directory, with the store's mode, renamed over the store. horch
/// reads the store again just before the rename and starts again, at most 3
/// times, when another process changed it. For Claude it holds Claude's own
/// lock, the directory `<store>.lock`, across the read and the rename. After
/// the write it reads the store again and fails when it does not trust the
/// workdir.
///
/// It never trusts `/`, the home directory or an ancestor of it, a relative
/// path, or a path that is not an existing directory. It never writes
/// through a symlinked store, never changes an explicit Codex
/// `trust_level = "untrusted"`, and never creates a Claude or Antigravity
/// store: those return [`TrustWrite::Skipped`]. `home`,
/// `claude_config_dir` and `codex_home_dir` are as in [`read_trust`].
pub fn ensure_trusted(
    harness: HarnessKind,
    workdir: &Path,
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
) -> anyhow::Result<TrustWrite> {
    ensure_trusted_with(
        harness,
        workdir,
        home,
        claude_config_dir,
        codex_home_dir,
        &mut || {},
    )
}

/// The stderr line for what [`ensure_trusted`] did, `None` when the store
/// already trusted `workdir`. It names the store file, never its content.
pub fn trust_note(
    harness: HarnessKind,
    workdir: &Path,
    outcome: &anyhow::Result<TrustWrite>,
) -> Option<String> {
    let name = harness.as_str();
    let dir = workdir.display();
    match outcome {
        Ok(TrustWrite::AlreadyTrusted) => None,
        Ok(TrustWrite::Written(file)) => Some(format!(
            "horch: trusted {dir} for {name} ({})",
            file.display()
        )),
        Ok(TrustWrite::Skipped(why)) => Some(format!(
            "horch: NOTE: horch did not trust {dir} for {name}: {why}"
        )),
        Err(e) => Some(format!(
            "horch: NOTE: horch could not trust {dir} for {name}: {e:#}"
        )),
    }
}

/// How many times a write starts again when the store changes under it.
const WRITE_TRIES: usize = 3;
/// A Claude lock directory older than this is stale (proper-lockfile's
/// default, which Claude uses).
const LOCK_STALE: Duration = Duration::from_secs(10);
/// How long horch waits for a live Claude lock before it skips.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// [`ensure_trusted`], calling `before_rename` after the new store is in its
/// temporary file and before the store is read again. A test changes the
/// store there.
fn ensure_trusted_with(
    harness: HarnessKind,
    workdir: &Path,
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
    before_rename: &mut dyn FnMut(),
) -> anyhow::Result<TrustWrite> {
    if !asks_for_trust(harness) {
        return Ok(TrustWrite::Skipped(format!(
            "{} has no trust step",
            harness.as_str()
        )));
    }
    let roots = match trust_roots(workdir, home) {
        Ok(roots) => roots,
        Err(why) => return Ok(TrustWrite::Skipped(why)),
    };
    let keys: Vec<String> = roots
        .iter()
        .map(|r| r.to_string_lossy().into_owned())
        .collect();
    let (file, label, locked) = match harness {
        HarnessKind::Claude => (
            claude_config_file(home, claude_config_dir),
            CLAUDE_TRUST_FILE,
            true,
        ),
        HarnessKind::Codex => (
            super::codex::codex_home(home, codex_home_dir).join("config.toml"),
            CODEX_TRUST_FILE,
            false,
        ),
        _ => (
            home.join(".gemini/antigravity-cli/settings.json"),
            ANTIGRAVITY_TRUST_FILE,
            false,
        ),
    };
    let verdict = |text: Option<&str>| match harness {
        HarnessKind::Claude => claude_trust(text, &roots),
        HarnessKind::Codex => codex_trust(text, &roots),
        _ => antigravity_trust(text, &roots),
    };
    let edit = |text: Option<&str>| -> Result<String, String> {
        match (harness, text) {
            (HarnessKind::Codex, text) => codex_add_trust(text.unwrap_or(""), &keys, label),
            (_, None) => Err(format!(
                "{label} does not exist: {} has never run with this store",
                harness.as_str()
            )),
            (HarnessKind::Claude, Some(text)) => claude_add_trust(text, &keys, label),
            (_, Some(text)) => antigravity_add_trust(text, &keys, label),
        }
    };
    for _ in 0..WRITE_TRIES {
        match std::fs::symlink_metadata(&file) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Ok(TrustWrite::Skipped(format!(
                    "{label} ({}) is a symlink; horch does not write through it",
                    file.display()
                )))
            }
            _ => {}
        }
        if harness == HarnessKind::Codex && !file.parent().is_some_and(Path::is_dir) {
            return Ok(TrustWrite::Skipped(format!(
                "the directory of {label} ({}) does not exist",
                file.display()
            )));
        }
        let _lock = if locked {
            match StoreLock::take(&file)? {
                Some(lock) => Some(lock),
                None => {
                    return Ok(TrustWrite::Skipped(format!(
                        "{}.lock stays held by another process",
                        file.display()
                    )))
                }
            }
        } else {
            None
        };
        let before = read_store(&file)
            .map_err(|()| anyhow::anyhow!("{label} ({}) cannot be read", file.display()))?;
        let found = verdict(before.as_deref());
        match found.state {
            TrustState::Trusted => return Ok(TrustWrite::AlreadyTrusted),
            TrustState::Unknown => return Ok(TrustWrite::Skipped(found.reason)),
            TrustState::Untrusted => {}
        }
        let after = match edit(before.as_deref()) {
            Ok(after) => after,
            Err(why) => return Ok(TrustWrite::Skipped(why)),
        };
        let temp = TempStore::write(&file, &after)?;
        before_rename();
        let now = read_store(&file)
            .map_err(|()| anyhow::anyhow!("{label} ({}) cannot be read", file.display()))?;
        if now != before {
            continue;
        }
        temp.rename_over(&file)?;
        drop(_lock);
        let written = read_store(&file)
            .map_err(|()| anyhow::anyhow!("{label} ({}) cannot be read", file.display()))?;
        if verdict(written.as_deref()).state != TrustState::Trusted {
            anyhow::bail!(
                "{label} ({}) does not trust {} after horch wrote it",
                file.display(),
                workdir.display()
            );
        }
        return Ok(TrustWrite::Written(file));
    }
    anyhow::bail!(
        "{label} ({}) changed during each of {WRITE_TRIES} tries; horch wrote nothing",
        file.display()
    )
}

/// `workdir` and its canonical form, when horch may trust them: an
/// absolute path of an existing directory that is not `/`, the home
/// directory or an ancestor of it. Else why not.
fn trust_roots(workdir: &Path, home: &Path) -> Result<Vec<PathBuf>, String> {
    let shown = workdir.display();
    if !workdir.is_absolute() {
        return Err(format!("{shown} is not an absolute path"));
    }
    if !workdir.is_dir() {
        return Err(format!("{shown} is not an existing directory"));
    }
    if !home.is_absolute() {
        return Err(format!(
            "the home directory is not known, so horch cannot protect it; {shown} is not trusted"
        ));
    }
    let mut roots = vec![workdir.to_path_buf()];
    if let Ok(canonical) = workdir.canonicalize() {
        if canonical != workdir {
            roots.push(canonical);
        }
    }
    let mut homes = vec![home.to_path_buf()];
    if let Ok(canonical) = home.canonicalize() {
        homes.push(canonical);
    }
    let refused = roots
        .iter()
        .any(|r| r.parent().is_none() || homes.iter().any(|h| h.starts_with(r)));
    if refused {
        return Err(format!(
            "{shown} is / or the home directory or an ancestor of it; horch never trusts those"
        ));
    }
    Ok(roots)
}

/// Claude's lock on its store: the directory `<store>.lock`, as
/// proper-lockfile makes it. Dropping the guard removes the directory.
struct StoreLock(PathBuf);

impl StoreLock {
    /// Take the lock. A lock directory older than [`LOCK_STALE`] is stale and
    /// is taken over. `None` when a live lock stays held for [`LOCK_WAIT`].
    fn take(store: &Path) -> anyhow::Result<Option<StoreLock>> {
        let mut dir = store.as_os_str().to_os_string();
        dir.push(".lock");
        let dir = PathBuf::from(dir);
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Some(StoreLock(dir))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&dir)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .is_some_and(|age| age > LOCK_STALE);
                    if stale && std::fs::remove_dir(&dir).is_ok() {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(anyhow::Error::new(e)
                        .context(format!("creating the lock {}", dir.display())))
                }
            }
        }
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// The new store text in a temporary file next to the store,
/// `<store>.tmp.<pid>.<12 hex>`, with the store's mode (`0600` for a new
/// store). Dropped without [`TempStore::rename_over`], it is removed.
struct TempStore(Option<PathBuf>);

impl TempStore {
    fn write(store: &Path, text: &str) -> anyhow::Result<TempStore> {
        use std::io::Write as _;
        let mut name = store.as_os_str().to_os_string();
        let random = uuid::Uuid::new_v4().simple().to_string();
        name.push(format!(".tmp.{}.{}", std::process::id(), &random[..12]));
        let path = PathBuf::from(name);
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create_new(true);
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
            let mode = std::fs::metadata(store)
                .map(|m| m.permissions().mode() & 0o7777)
                .unwrap_or(0o600);
            open.mode(mode);
            mode
        };
        let mut f = open
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        let temp = TempStore(Some(path));
        let path = temp.0.as_deref().unwrap_or(Path::new(""));
        // The umask can narrow the mode `open` asked for.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("setting the mode of {}", path.display()))?;
        }
        f.write_all(text.as_bytes())
            .and_then(|()| f.sync_all())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(temp)
    }

    fn rename_over(mut self, store: &Path) -> anyhow::Result<()> {
        let path = self.0.take().unwrap_or_default();
        if let Err(e) = std::fs::rename(&path, store) {
            let _ = std::fs::remove_file(&path);
            return Err(anyhow::Error::new(e).context(format!(
                "renaming {} over {}",
                path.display(),
                store.display()
            )));
        }
        Ok(())
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `text` with `projects["<key>"].hasTrustDialogAccepted` set to `true`
/// for each key. Only those bytes change: horch inserts members or replaces
/// one value, and never re-serializes the file, so no other value can change
/// (a re-parsed float could). Err: why horch cannot edit it safely.
fn claude_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    let unsafe_edit = || format!("{label} has a shape horch does not edit");
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|_| format!("{label} is not valid JSON"))?;
    let mut out = text.to_string();
    for key in keys {
        let top = json_object_at(&out, 0).ok_or_else(unsafe_edit)?;
        let accepted = r#""hasTrustDialogAccepted":true"#;
        let entry = format!("{}:{{{accepted}}}", json_quote(key));
        let Some(projects) = json_member(&top, "projects") else {
            out = json_insert(&out, &top, &format!("\"projects\":{{{entry}}}"));
            continue;
        };
        let projects = json_object_at(&out, projects.0).ok_or_else(unsafe_edit)?;
        let Some(project) = json_member(&projects, key) else {
            out = json_insert(&out, &projects, &entry);
            continue;
        };
        let project = json_object_at(&out, project.0).ok_or_else(unsafe_edit)?;
        match json_member(&project, "hasTrustDialogAccepted") {
            None => out = json_insert(&out, &project, accepted),
            Some((start, end)) => out.replace_range(start..end, "true"),
        }
    }
    for key in keys {
        let projects = expected
            .as_object_mut()
            .ok_or_else(unsafe_edit)?
            .entry("projects")
            .or_insert_with(|| serde_json::json!({}));
        let project = projects
            .as_object_mut()
            .ok_or_else(unsafe_edit)?
            .entry(key.clone())
            .or_insert_with(|| serde_json::json!({}));
        project
            .as_object_mut()
            .ok_or_else(unsafe_edit)?
            .insert("hasTrustDialogAccepted".into(), true.into());
    }
    json_check(&out, &expected).ok_or_else(unsafe_edit)
}

/// `text` with each key that is not in `trustedWorkspaces` added to the
/// front of that list. Only those bytes change, as in [`claude_add_trust`].
fn antigravity_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    let unsafe_edit = || format!("{label} has a shape horch does not edit");
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|_| format!("{label} is not valid JSON"))?;
    let list = expected
        .as_object_mut()
        .ok_or_else(unsafe_edit)?
        .entry("trustedWorkspaces")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(unsafe_edit)?;
    let missing: Vec<&String> = keys
        .iter()
        .filter(|k| !list.iter().any(|d| d.as_str() == Some(k.as_str())))
        .collect();
    for (i, key) in missing.iter().enumerate() {
        list.insert(i, serde_json::Value::String((*key).clone()));
    }
    let items = missing
        .iter()
        .map(|k| json_quote(k))
        .collect::<Vec<_>>()
        .join(",");
    let top = json_object_at(text, 0).ok_or_else(unsafe_edit)?;
    let out = match json_member(&top, "trustedWorkspaces") {
        None => json_insert(text, &top, &format!("\"trustedWorkspaces\":[{items}]")),
        Some((start, _)) => {
            let t = text.as_bytes();
            if t.get(start) != Some(&b'[') {
                return Err(unsafe_edit());
            }
            let empty = t.get(json_ws(t, start + 1)) == Some(&b']');
            let sep = if empty { "" } else { "," };
            format!("{}{items}{sep}{}", &text[..=start], &text[start + 1..])
        }
    };
    json_check(&out, &expected).ok_or_else(unsafe_edit)
}

/// `out` when it parses to `expected`; a guard on the byte edits.
fn json_check(out: &str, expected: &serde_json::Value) -> Option<String> {
    let got: serde_json::Value = serde_json::from_str(out).ok()?;
    (&got == expected).then(|| out.to_string())
}

/// `text` as a JSON string.
fn json_quote(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// The span of the object whose `{` is the first non-space byte at or
/// after `at`: the offset of `{` and the members, as (key, value start,
/// value end). `None` when it is not an object.
fn json_object_at(text: &str, at: usize) -> Option<JsonObject> {
    let t = text.as_bytes();
    let open = json_ws(t, at);
    if t.get(open) != Some(&b'{') {
        return None;
    }
    let mut members = Vec::new();
    let mut i = json_ws(t, open + 1);
    if t.get(i) == Some(&b'}') {
        return Some(JsonObject { open, members });
    }
    loop {
        if t.get(i) != Some(&b'"') {
            return None;
        }
        let key_end = json_string_end(t, i)?;
        let key: String = serde_json::from_str(&text[i..key_end]).ok()?;
        i = json_ws(t, key_end);
        if t.get(i) != Some(&b':') {
            return None;
        }
        let start = json_ws(t, i + 1);
        let end = json_value_end(t, start)?;
        members.push((key, start, end));
        i = json_ws(t, end);
        match t.get(i)? {
            b',' => i = json_ws(t, i + 1),
            b'}' => return Some(JsonObject { open, members }),
            _ => return None,
        }
    }
}

/// An object in a JSON text: the offset of its `{` and its members.
struct JsonObject {
    open: usize,
    members: Vec<(String, usize, usize)>,
}

/// The value span of `key` in `object`. The last one wins, as when the
/// file is parsed.
fn json_member(object: &JsonObject, key: &str) -> Option<(usize, usize)> {
    object
        .members
        .iter()
        .rev()
        .find(|(k, _, _)| k == key)
        .map(|&(_, start, end)| (start, end))
}

/// `text` with `member` (`"key":value`) first in `object`.
fn json_insert(text: &str, object: &JsonObject, member: &str) -> String {
    let sep = if object.members.is_empty() { "" } else { "," };
    let at = object.open + 1;
    format!("{}{member}{sep}{}", &text[..at], &text[at..])
}

fn json_ws(t: &[u8], mut i: usize) -> usize {
    while matches!(t.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// The end (past the closing quote) of the string that starts at `i`.
fn json_string_end(t: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    loop {
        match t.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
}

/// The end of the value that starts at `i`, in text that is valid JSON.
fn json_value_end(t: &[u8], i: usize) -> Option<usize> {
    match t.get(i)? {
        b'"' => json_string_end(t, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            loop {
                match t.get(j)? {
                    b'"' => {
                        j = json_string_end(t, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        _ => {
            let mut j = i;
            while t
                .get(j)
                .is_some_and(|b| !matches!(b, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'))
            {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// `text` with a `[projects."<key>"]` table, `trust_level = "trusted"`, for
/// each key appended. The text before it stays byte for byte. Err when a
/// table for a key exists already: an explicit `trust_level = "untrusted"`
/// is the operator's choice, and any other table horch does not edit. Err
/// too when the file uses a `projects` form other than one table per path.
fn codex_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    for line in text.lines().map(str::trim) {
        let other_form = line.starts_with("[projects]")
            || line.starts_with("[[projects")
            || (line.starts_with("projects")
                && line["projects".len()..]
                    .trim_start()
                    .starts_with(['=', '.']));
        if other_form {
            return Err(format!(
                "{label} keeps projects in a form horch does not edit; trust the path in codex"
            ));
        }
    }
    let mut table: Option<String> = None;
    let mut levels: BTreeMap<String, Option<String>> = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            table = toml_project_header(line).filter(|p| keys.contains(p));
            if let Some(path) = &table {
                levels.entry(path.clone()).or_insert(None);
            }
        } else if let Some(path) = &table {
            if let Some(level) = toml_string_value(line, "trust_level") {
                levels.insert(path.clone(), Some(level));
            }
        }
    }
    if let Some((path, _)) = levels
        .iter()
        .find(|(_, level)| level.as_deref() == Some("untrusted"))
    {
        return Err(format!(
            "{label} sets trust_level = \"untrusted\" for {path}; horch keeps the operator's choice"
        ));
    }
    if let Some(path) = levels.keys().next() {
        return Err(format!(
            "{label} has a [projects.\"{path}\"] table without trust_level = \"trusted\"; horch does not edit it"
        ));
    }
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    for key in keys {
        if key.chars().any(char::is_control) {
            return Err(format!("{label}: the path {key:?} has a control character"));
        }
        let quoted = key.replace('\\', "\\\\").replace('"', "\\\"");
        let header = format!("[projects.\"{quoted}\"]");
        if toml_project_header(&header).as_deref() != Some(key.as_str()) {
            return Err(format!("{label}: horch cannot quote the path {key:?}"));
        }
        out.push_str(&header);
        out.push_str("\ntrust_level = \"trusted\"\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp home with a project directory in it. Nothing here touches the
    /// real home.
    struct Fake {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        project: PathBuf,
    }

    impl Fake {
        fn new() -> Fake {
            let tmp = tempfile::tempdir().unwrap();
            // Canonical, so the workdir has one form and one key.
            let root = tmp.path().canonicalize().unwrap();
            let home = root.join("home");
            let project = home.join("projects/alpha");
            std::fs::create_dir_all(&project).unwrap();
            Fake {
                _tmp: tmp,
                home,
                project,
            }
        }

        fn claude_file(&self) -> PathBuf {
            self.home.join(".claude.json")
        }

        fn codex_file(&self) -> PathBuf {
            self.home.join(".codex/config.toml")
        }

        fn ensure(&self, harness: HarnessKind, workdir: &Path) -> TrustWrite {
            ensure_trusted(harness, workdir, &self.home, None, None).unwrap()
        }

        fn state(&self, harness: HarnessKind) -> TrustState {
            read_trust(
                &[harness],
                &self.home,
                None,
                None,
                std::slice::from_ref(&self.project),
            )[0]
            .state
        }
    }

    fn json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn arc_29_claude_entry_written_when_missing_and_other_keys_kept() {
        let f = Fake::new();
        let other = f.home.join("projects/beta");
        let before = serde_json::json!({
            "numStartups": 42,
            "oauthAccount": {"emailAddress": "a@b.invalid", "token": "SECRET"},
            "cachedRatio": 0.25,
            "big": 123456789012345678901234567890u128.to_string(),
            "projects": {
                other.to_string_lossy(): {"hasTrustDialogAccepted": true, "allowedTools": ["Bash"]}
            }
        });
        let text = serde_json::to_string_pretty(&before).unwrap();
        std::fs::write(f.claude_file(), &text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.claude_file(), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        assert_eq!(f.state(HarnessKind::Claude), TrustState::Untrusted);

        let wrote = f.ensure(HarnessKind::Claude, &f.project);
        assert_eq!(wrote, TrustWrite::Written(f.claude_file()));
        assert_eq!(f.state(HarnessKind::Claude), TrustState::Trusted);
        let mut expected = before.clone();
        expected["projects"][f.project.to_string_lossy().as_ref()] =
            serde_json::json!({"hasTrustDialogAccepted": true});
        assert_eq!(json(&f.claude_file()), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(f.claude_file())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // No temp file and no lock directory stays behind.
        let mut left: Vec<_> = std::fs::read_dir(&f.home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".claude.json", "projects"]);

        // An entry for the project without the key gets the key, and keeps
        // its other keys.
        let mut partial = before.clone();
        partial["projects"][f.project.to_string_lossy().as_ref()] =
            serde_json::json!({"allowedTools": ["Read"], "hasTrustDialogAccepted": false});
        std::fs::write(f.claude_file(), partial.to_string()).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        partial["projects"][f.project.to_string_lossy().as_ref()]["hasTrustDialogAccepted"] =
            serde_json::Value::Bool(true);
        assert_eq!(json(&f.claude_file()), partial);

        // A number serde_json cannot round-trip keeps its bytes: horch edits
        // the text, it does not re-serialize it.
        let long = r#"{"ratio": 0.12345678901234567890123, "big": 123456789012345678901234567890}"#;
        std::fs::write(f.claude_file(), long).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        let after = std::fs::read_to_string(f.claude_file()).unwrap();
        assert!(after.contains("0.12345678901234567890123"), "{after}");
        assert!(after.contains("123456789012345678901234567890"), "{after}");
    }

    #[test]
    fn arc_29_claude_already_trusted_writes_nothing() {
        let f = Fake::new();
        let text = serde_json::json!({"projects": {
            f.project.to_string_lossy(): {"hasTrustDialogAccepted": true}
        }})
        .to_string();
        std::fs::write(f.claude_file(), &text).unwrap();
        let before = std::fs::metadata(f.claude_file())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::AlreadyTrusted
        );
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), text);
        let after = std::fs::metadata(f.claude_file())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);

        // No file: Claude never ran with this config dir. Skip, write nothing.
        std::fs::remove_file(f.claude_file()).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("does not exist"), "{why}");
        assert!(!f.claude_file().exists());
    }

    #[cfg(unix)]
    #[test]
    fn arc_29_claude_symlinked_store_is_skipped() {
        let f = Fake::new();
        let real = f.home.join("dotfiles-claude.json");
        std::fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, f.claude_file()).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("symlink"), "{why}");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{}");
    }

    #[test]
    fn arc_29_codex_table_appended_and_rest_of_file_byte_identical() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        // No final newline, a comment, odd spacing: all kept byte for byte.
        let text = "model = \"gpt-5\"  # mine\n\n[projects.\"/elsewhere\"]\ntrust_level = \"trusted\"\n[tui]\nx=1";
        std::fs::write(f.codex_file(), text).unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::Written(f.codex_file())
        );
        let after = std::fs::read_to_string(f.codex_file()).unwrap();
        assert!(after.starts_with(text), "{after}");
        assert_eq!(
            &after[text.len()..],
            format!(
                "\n\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                f.project.display()
            )
        );
        assert_eq!(f.state(HarnessKind::Codex), TrustState::Trusted);
        // A second launch writes nothing.
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::AlreadyTrusted
        );
        assert_eq!(std::fs::read_to_string(f.codex_file()).unwrap(), after);

        // No file: horch creates it with only the table.
        std::fs::remove_file(f.codex_file()).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::Written(_)
        ));
        assert_eq!(
            std::fs::read_to_string(f.codex_file()).unwrap(),
            format!(
                "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                f.project.display()
            )
        );
    }

    #[test]
    fn arc_29_codex_explicit_untrusted_is_kept() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        let text = format!(
            "[projects.\"{}\"]\ntrust_level = \"untrusted\"\n",
            f.project.display()
        );
        std::fs::write(f.codex_file(), &text).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Codex, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("untrusted"), "{why}");
        assert!(
            why.contains(&f.project.to_string_lossy().into_owned()),
            "{why}"
        );
        assert!(why.contains("config.toml"), "{why}");
        assert_eq!(std::fs::read_to_string(f.codex_file()).unwrap(), text);
    }

    #[test]
    fn arc_29_antigravity_workspace_added_and_other_keys_kept() {
        let f = Fake::new();
        let file = f.home.join(".gemini/antigravity-cli/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"theme": "dark", "trustedWorkspaces": ["/other"]}"#,
        )
        .unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Antigravity, &f.project),
            TrustWrite::Written(file.clone())
        );
        assert_eq!(f.state(HarnessKind::Antigravity), TrustState::Trusted);
        assert_eq!(
            json(&file),
            serde_json::json!({
                "theme": "dark",
                "trustedWorkspaces": [f.project.to_string_lossy(), "/other"]
            })
        );
    }

    #[test]
    fn arc_29_home_and_its_ancestors_are_never_trusted() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), "{}").unwrap();
        let mut refused = vec![f.home.clone(), PathBuf::from("/")];
        refused.extend(f.home.ancestors().skip(1).map(Path::to_path_buf));
        for dir in &refused {
            for harness in [
                HarnessKind::Claude,
                HarnessKind::Codex,
                HarnessKind::Antigravity,
            ] {
                let got = f.ensure(harness, dir);
                assert!(
                    matches!(got, TrustWrite::Skipped(_)),
                    "{dir:?} {harness:?}: {got:?}"
                );
            }
        }
        // A path that is not an existing directory, or is relative.
        for dir in [
            f.home.join("missing"),
            f.claude_file(),
            PathBuf::from("rel"),
        ] {
            let got = f.ensure(HarnessKind::Claude, &dir);
            assert!(matches!(got, TrustWrite::Skipped(_)), "{dir:?}: {got:?}");
        }
        // The home through a symlink is still the home.
        #[cfg(unix)]
        {
            let link = f.project.join("home-link");
            std::os::unix::fs::symlink(&f.home, &link).unwrap();
            assert!(matches!(
                f.ensure(HarnessKind::Claude, &link),
                TrustWrite::Skipped(_)
            ));
        }
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), "{}");
        assert!(!f.codex_file().exists());
        assert!(!f.home.join(".gemini").exists());
    }

    #[test]
    fn arc_29_a_concurrent_write_is_not_lost() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), r#"{"numStartups": 1}"#).unwrap();
        let file = f.claude_file();
        let mut calls = 0;
        let wrote = ensure_trusted_with(
            HarnessKind::Claude,
            &f.project,
            &f.home,
            None,
            None,
            &mut || {
                calls += 1;
                if calls == 1 {
                    // Another Claude session saves its config between horch's
                    // read and its rename.
                    std::fs::write(&file, r#"{"numStartups": 2, "theirs": true}"#).unwrap();
                }
            },
        )
        .unwrap();
        assert_eq!(wrote, TrustWrite::Written(f.claude_file()));
        assert_eq!(calls, 2, "horch reads again after the other write");
        assert_eq!(
            json(&f.claude_file()),
            serde_json::json!({
                "numStartups": 2,
                "theirs": true,
                "projects": {f.project.to_string_lossy(): {"hasTrustDialogAccepted": true}}
            })
        );

        // A store that changes on every try fails after 3 tries, and leaves
        // the other writer's content.
        std::fs::write(f.claude_file(), "{}").unwrap();
        let mut n = 0;
        let err = ensure_trusted_with(
            HarnessKind::Claude,
            &f.project,
            &f.home,
            None,
            None,
            &mut || {
                n += 1;
                std::fs::write(&file, format!("{{\"n\": {n}}}")).unwrap();
            },
        )
        .unwrap_err();
        assert_eq!(n, 3);
        assert!(format!("{err:#}").contains("changed"), "{err:#}");
        assert_eq!(json(&f.claude_file()), serde_json::json!({"n": 3}));
    }

    #[test]
    fn arc_29_a_held_claude_lock_is_waited_for_and_a_stale_one_is_taken() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), "{}").unwrap();
        let lock = f.home.join(".claude.json.lock");
        // A stale lock (older than 10 s): horch takes it and removes it after.
        std::fs::create_dir(&lock).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        assert!(!lock.exists());
        // A fresh lock that stays held: horch skips, writes nothing.
        std::fs::write(f.claude_file(), "{}").unwrap();
        std::fs::create_dir(&lock).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains(".claude.json.lock"), "{why}");
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), "{}");
        assert!(lock.exists(), "horch never removes a live lock");
    }

    #[test]
    fn arc_29_harnesses_without_trust_are_skipped() {
        let f = Fake::new();
        for harness in [HarnessKind::OpenCode, HarnessKind::Pi, HarnessKind::Prime] {
            assert!(matches!(
                f.ensure(harness, &f.project),
                TrustWrite::Skipped(_)
            ));
        }
    }
}
