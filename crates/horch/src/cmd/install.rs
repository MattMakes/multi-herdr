//! `horch install` - replaces `scripts/install-herdr-fleet.mjs`.
//!
//! The Node script had to inject a shell function into `~/.zshrc` or the
//! PowerShell profile, because the real entry point was
//! `just --justfile <repo>/justfile herdr-fleet` and so depended on the repo
//! staying put. `horch` embeds its prompts and needs no repo, so installing is
//! just putting one binary on PATH - no profile editing, nothing to keep in sync.
//! The dataset binary, `multi-herdr-dataset`, and `herdr-install` go next to it,
//! and so do the `herdr-fleet` and `herdr-run` launchers (unix only).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_core::runtime::{process, RuntimeContext};

/// Where to put the binary when the caller does not say.
pub fn default_dir(ctx: &RuntimeContext) -> PathBuf {
    if cfg!(windows) {
        ctx.inherited
            .local_app_data
            .as_ref()
            .map(|base| Path::new(base).join("Programs").join("horch"))
            .unwrap_or_else(|| ctx.paths.home.join("horch"))
    } else {
        ctx.paths.home.join(".local").join("bin")
    }
}

pub fn install(ctx: &RuntimeContext, dir: Option<&str>) -> Result<()> {
    let dir = dir.map(PathBuf::from).unwrap_or_else(|| default_dir(ctx));
    let source = ctx
        .bins
        .current_exe
        .clone()
        .context("locating the running horch binary")?;
    let file_name = source
        .file_name()
        .context("the running binary has no file name")?;
    let target = dir.join(file_name);

    if cfg!(windows) {
        // A process that still runs from a `.old` file keeps it; that is fine.
        for old in stale_old_paths(&dir) {
            let _ = std::fs::remove_file(old);
        }
    }
    install_one("horch", &source, &target)?;
    // A live collector keeps running the binary it started with: old
    // prices, old readers. Restart it on the binary just installed.
    let herdr = horch_core::workspace::herdr::Herdr::with_bin(&ctx.bins.harness.herdr);
    if let Err(e) = super::telemetry::restart_if_stale(ctx, &herdr, &target) {
        println!("warning: telemetry collector not restarted: {e:#}; run `horch telemetry ensure`");
    }

    // The sibling binaries are built next to horch (`cargo build --bin
    // multi-herdr-dataset --bin herdr-install`), and are installed next to it too.
    install_siblings(&source, &dir)?;
    install_launchers(&dir, ctx.bins.roster_override.as_deref())?;

    if ctx
        .inherited
        .path
        .as_ref()
        .is_some_and(|path| process::on_path_in(path, &dir))
    {
        println!("\nReady. cd into a project and run: horch fleet");
        println!("(needs a running herdr server: launch the herdr app, or `herdr server`)");
    } else {
        println!("\n{} is not on your PATH. Add it:\n", dir.display());
        if cfg!(windows) {
            println!("  PowerShell (current session):");
            println!("    $env:Path = \"{};$env:Path\"", dir.display());
            println!("\n  Permanently, for future sessions:");
            println!("    setx PATH \"{};$($env:Path)\"", dir.display());
        } else {
            println!("  Add to ~/.zshrc (or ~/.bashrc):");
            println!("    export PATH=\"{}:$PATH\"", dir.display());
        }
        println!("\nThen open a new terminal, cd into a project, and run: horch fleet");
    }

    println!("\nInstalled version: {}", installed_version(&target)?);
    Ok(())
}

/// The second binary that `horch install` puts on PATH.
pub const DATASET_BIN: &str = "multi-herdr-dataset";

/// The third binary. It is optional for a source build.
pub const HERDR_INSTALL_BIN: &str = "herdr-install";

/// The launcher scripts, with `__TEAMMATES_DIR__` as the roster placeholder.
const FLEET_LAUNCHER: &str = include_str!("../../../../scripts/herdr-fleet");
const RUN_LAUNCHER: &str = include_str!("../../../../scripts/herdr-run");

pub fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Installs each binary built beside `horch_source` into `dir`. A missing
/// binary prints one line and the install goes on.
fn install_siblings(horch_source: &Path, dir: &Path) -> Result<()> {
    for bin in [DATASET_BIN, HERDR_INSTALL_BIN] {
        let source = sibling(horch_source, bin);
        if source.is_file() {
            install_one(bin, &source, &dir.join(exe_name(bin)))?;
        } else {
            println!(
                "{bin} is not built next to horch ({}); build it with `cargo build --release --bin {bin}`, then install again",
                source.display()
            );
        }
    }
    Ok(())
}

/// Writes the `herdr-fleet` and `herdr-run` launchers into `dir`. They are
/// shell scripts, so Windows gets none: `horch fleet` is the entry point there.
fn install_launchers(dir: &Path, teammates: Option<&Path>) -> Result<()> {
    if !cfg!(unix) {
        println!(
            "the herdr-fleet and herdr-run launchers are unix-only; run `horch fleet` instead"
        );
        return Ok(());
    }
    for (name, template) in [("herdr-fleet", FLEET_LAUNCHER), ("herdr-run", RUN_LAUNCHER)] {
        let target = dir.join(name);
        let body = render_launcher(template, teammates);
        replace_file(&target, |staged| {
            std::fs::write(staged, &body).with_context(|| format!("writing {}", staged.display()))
        })?;
        println!("installed {name} to {}", target.display());
    }
    Ok(())
}

/// The roster assignment line of a launcher template.
const TEAMMATES_LINE: &str = "HORCH_TEAMMATES_DIR=\"__TEAMMATES_DIR__\"";

/// `text` as a POSIX single-quoted literal: no expansion happens inside it.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Fills in a launcher template. With a roster dir the path is baked in; with
/// none the two `HORCH_TEAMMATES_DIR` lines go, and horch uses its own roster.
fn render_launcher(template: &str, teammates: Option<&Path>) -> String {
    match teammates {
        Some(dir) => {
            let line = format!(
                "HORCH_TEAMMATES_DIR={}",
                shell_quote(&dir.to_string_lossy())
            );
            template.replace(TEAMMATES_LINE, &line)
        }
        None => template
            .lines()
            .filter(|line| !line.contains("HORCH_TEAMMATES_DIR"))
            .map(|line| format!("{line}\n"))
            .collect(),
    }
}

/// The binary `name` built beside `horch_source`. A link to horch is
/// resolved first, so the sibling is the one in the build directory.
fn sibling(horch_source: &Path, name: &str) -> PathBuf {
    let real = horch_source
        .canonicalize()
        .unwrap_or_else(|_| horch_source.to_path_buf());
    real.with_file_name(exe_name(name))
}

/// Install `source` as `target`, unless that real file is there already.
fn install_one(name: &str, source: &Path, target: &Path) -> Result<()> {
    // `same_file` follows symlinks, so a link that resolves to this binary
    // would count as installed. The install must be a real file: not a link.
    if same_file(source, target) && !is_symlink(target) {
        println!("{name} is already installed at {}", target.display());
    } else {
        place_binary(source, target)?;
        println!("installed {name} to {}", target.display());
    }
    Ok(())
}

/// Copies `source` to `target`, replacing whatever is there.
fn place_binary(source: &Path, target: &Path) -> Result<()> {
    // The running binary can be reached through the very symlink removed below,
    // so resolve it to the real file first.
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    replace_file(target, |staged| {
        std::fs::copy(&source, staged)
            .with_context(|| format!("copying {} to {}", source.display(), staged.display()))?;
        Ok(())
    })
}

/// Fills a staged file with `fill`, makes it executable, and moves it to
/// `target`. Replace rather than write in place: overwriting a running binary
/// fails on Windows and can crash a live process on Unix.
fn replace_file(target: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let dir = target
        .parent()
        .context("the install path has no parent directory")?;
    let file_name = target
        .file_name()
        .context("the install path has no file name")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    remove_symlink(target)?;
    let staged = dir.join(format!(".{}.new", file_name.to_string_lossy()));
    fill(&staged)?;
    process::make_executable(&staged)?;
    // A running exe cannot be overwritten on Windows, but it can be renamed.
    swap_in(&staged, target, cfg!(windows), |from, to| {
        std::fs::rename(from, to)
    })
}

/// Moves `staged` to `target`. With `move_aside`, an existing `target` goes to
/// its `.old` path first, and comes back if the final rename fails. `rename`
/// is a parameter so a test can make it fail.
fn swap_in(
    staged: &Path,
    target: &Path,
    move_aside: bool,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let mut aside = None;
    if move_aside && target.exists() {
        let old = old_path(target);
        let _ = std::fs::remove_file(&old);
        rename(target, &old)
            .with_context(|| format!("moving {} aside to {}", target.display(), old.display()))?;
        aside = Some(old);
    }
    let Err(err) = rename(staged, target) else {
        return Ok(());
    };
    let err = anyhow::Error::new(err).context(format!(
        "installing to {} (is a horch process running from there?)",
        target.display()
    ));
    let Some(old) = aside else {
        return Err(err);
    };
    match rename(&old, target) {
        Ok(()) => Err(err.context(format!("restored the previous {}", target.display()))),
        Err(rollback) => Err(err.context(format!(
            "restoring {} from {} also failed: {rollback}",
            target.display(),
            old.display()
        ))),
    }
}

/// The `.old` files that `install` removes at its start: one per installed
/// binary in `dir`. Windows leaves them after a replacement.
fn stale_old_paths(dir: &Path) -> Vec<PathBuf> {
    ["horch", DATASET_BIN, HERDR_INSTALL_BIN]
        .iter()
        .map(|bin| old_path(&dir.join(exe_name(bin))))
        .collect()
}

/// Where Windows moves a replaced exe: `.<file name>.old` in the same dir.
fn old_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.old"))
}

/// Removes `path` when it is a symlink, and says so. A link at the destination
/// is usually a stale one to a script in another checkout, and the install must
/// replace the link and never the file it points to.
fn remove_symlink(path: &Path) -> Result<()> {
    if !is_symlink(path) {
        return Ok(());
    }
    let points_to = std::fs::read_link(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "an unreadable path".to_string());
    std::fs::remove_file(path)
        .with_context(|| format!("removing the symlink {}", path.display()))?;
    println!(
        "removed the symlink {} (it pointed to {}) and replaced it with a real file",
        path.display(),
        points_to
    );
    Ok(())
}

/// True when `path` is itself a symlink, whether or not its target exists.
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Runs `<target> --version`. That reports the installed version and proves
/// the installed file runs.
fn installed_version(target: &Path) -> Result<String> {
    let out = std::process::Command::new(target)
        .arg("--version")
        .output()
        .with_context(|| format!("running {} --version", target.display()))?;
    anyhow::ensure!(
        out.status.success(),
        "{} --version exited with {}",
        target.display(),
        out.status
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when both paths name the same existing file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_platform_appropriate() {
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HOME", "/h")
                .with("USERPROFILE", "/h"),
        )
        .unwrap();
        let dir = default_dir(&ctx);
        if cfg!(windows) {
            assert!(dir.ends_with("horch"), "{}", dir.display());
        } else {
            assert!(dir.ends_with(".local/bin"), "{}", dir.display());
        }
    }

    #[test]
    fn same_file_needs_both_paths_to_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::write(&a, "x").unwrap();
        assert!(same_file(&a, &a));
        assert!(!same_file(&a, &tmp.path().join("missing")));
    }

    #[cfg(unix)]
    #[test]
    fn place_binary_replaces_a_symlink_and_leaves_its_target_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let old_script = tmp.path().join("old-script");
        std::fs::write(&old_script, "#!/bin/sh\necho old\n").unwrap();
        let source = tmp.path().join("new-binary");
        std::fs::write(&source, "new").unwrap();
        let target = tmp.path().join("bin").join("horch");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&old_script, &target).unwrap();

        place_binary(&source, &target).unwrap();

        assert!(!is_symlink(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(&old_script).unwrap(),
            "#!/bin/sh\necho old\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn place_binary_works_when_the_source_is_reached_through_the_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real-binary");
        std::fs::write(&real, "binary").unwrap();
        let target = tmp.path().join("bin").join("horch");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &target).unwrap();

        // Running `<target> install` reports the link itself as the source.
        place_binary(&target, &target).unwrap();

        assert!(!is_symlink(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "binary");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "binary");
    }

    #[test]
    fn place_binary_leaves_no_staged_file_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("new-binary");
        std::fs::write(&source, "new").unwrap();
        let target = tmp.path().join("bin").join("horch");

        place_binary(&source, &target).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("horch")]);
    }

    #[test]
    fn install_puts_the_dataset_binary_next_to_horch() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("release");
        std::fs::create_dir_all(&build).unwrap();
        let horch = build.join(exe_name("horch"));
        std::fs::write(&horch, "horch").unwrap();
        std::fs::write(build.join(exe_name(DATASET_BIN)), "dataset").unwrap();
        let bin = tmp.path().join("bin");

        let source = sibling(&horch, DATASET_BIN);
        install_one(DATASET_BIN, &source, &bin.join(exe_name(DATASET_BIN))).unwrap();

        assert_eq!(
            std::fs::read_to_string(bin.join(exe_name(DATASET_BIN))).unwrap(),
            "dataset"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sibling_follows_a_link_to_horch() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("release");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("horch"), "horch").unwrap();
        let link = tmp.path().join("horch-link");
        std::os::unix::fs::symlink(build.join("horch"), &link).unwrap();

        assert_eq!(
            sibling(&link, DATASET_BIN),
            build.canonicalize().unwrap().join(DATASET_BIN)
        );
    }

    fn build_dir(tmp: &Path, bins: &[&str]) -> PathBuf {
        let build = tmp.join("release");
        std::fs::create_dir_all(&build).unwrap();
        for bin in bins {
            std::fs::write(build.join(exe_name(bin)), *bin).unwrap();
        }
        build.join(exe_name("horch"))
    }

    #[test]
    fn install_siblings_installs_both_sibling_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let horch = build_dir(tmp.path(), &["horch", DATASET_BIN, HERDR_INSTALL_BIN]);
        let bin = tmp.path().join("bin");

        install_siblings(&horch, &bin).unwrap();

        for name in [DATASET_BIN, HERDR_INSTALL_BIN] {
            assert_eq!(
                std::fs::read_to_string(bin.join(exe_name(name))).unwrap(),
                name
            );
        }
    }

    #[test]
    fn install_siblings_goes_on_without_herdr_install() {
        let tmp = tempfile::tempdir().unwrap();
        let horch = build_dir(tmp.path(), &["horch", DATASET_BIN]);
        let bin = tmp.path().join("bin");

        install_siblings(&horch, &bin).unwrap();

        assert!(bin.join(exe_name(DATASET_BIN)).is_file());
        assert!(!bin.join(exe_name(HERDR_INSTALL_BIN)).exists());
    }

    #[test]
    fn render_launcher_bakes_in_the_roster_dir() {
        for template in [FLEET_LAUNCHER, RUN_LAUNCHER] {
            let out = render_launcher(template, Some(Path::new("/r/teammates")));
            assert!(
                out.contains("HORCH_TEAMMATES_DIR='/r/teammates'\n"),
                "{out}"
            );
            assert!(out.contains("export HORCH_TEAMMATES_DIR"));
            assert!(!out.contains("__TEAMMATES_DIR__"));
        }
    }

    #[test]
    fn render_launcher_without_a_roster_dir_sets_none() {
        for template in [FLEET_LAUNCHER, RUN_LAUNCHER] {
            let out = render_launcher(template, None);
            assert!(!out.contains("__TEAMMATES_DIR__"), "{out}");
            assert!(!out.contains("HORCH_TEAMMATES_DIR"), "{out}");
            assert!(out.starts_with("#!/bin/sh\n"));
            assert!(out.contains("exec horch fleet"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn install_launchers_writes_executable_scripts() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");

        install_launchers(&bin, Some(Path::new("/r/teammates"))).unwrap();

        for (name, template) in [("herdr-fleet", FLEET_LAUNCHER), ("herdr-run", RUN_LAUNCHER)] {
            let path = bin.join(name);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                render_launcher(template, Some(Path::new("/r/teammates")))
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o755,
                0o755
            );
        }
        assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 2);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("/a b"), "'/a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[cfg(unix)]
    #[test]
    fn render_launcher_quotes_a_roster_path_with_shell_characters() {
        let path = "/r/$HOME/`id`/\"q\"/it's\\x";
        for template in [FLEET_LAUNCHER, RUN_LAUNCHER] {
            let out = render_launcher(template, Some(Path::new(path)));
            let line = out
                .lines()
                .find(|l| l.starts_with("HORCH_TEAMMATES_DIR="))
                .unwrap();
            let script = format!("{line}; printf %s \"$HORCH_TEAMMATES_DIR\"");
            let got = std::process::Command::new("sh")
                .args(["-c", &script])
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&got.stdout), path);
        }
    }

    #[test]
    fn stale_old_paths_name_the_3_binaries() {
        assert_eq!(
            stale_old_paths(Path::new("/d")),
            vec![
                PathBuf::from(format!("/d/.horch{}.old", std::env::consts::EXE_SUFFIX)),
                PathBuf::from(format!(
                    "/d/.{DATASET_BIN}{}.old",
                    std::env::consts::EXE_SUFFIX
                )),
                PathBuf::from(format!(
                    "/d/.{HERDR_INSTALL_BIN}{}.old",
                    std::env::consts::EXE_SUFFIX
                )),
            ]
        );
    }

    fn swap_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("horch");
        let staged = tmp.path().join(".horch.new");
        std::fs::write(&target, "old").unwrap();
        std::fs::write(&staged, "new").unwrap();
        (tmp, staged, target)
    }

    #[test]
    fn swap_in_with_move_aside_replaces_the_target_and_keeps_old() {
        let (_tmp, staged, target) = swap_fixture();

        swap_in(&staged, &target, true, |from, to| std::fs::rename(from, to)).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_path(&target)).unwrap(), "old");
    }

    #[test]
    fn swap_in_restores_the_old_target_when_the_final_rename_fails() {
        let (_tmp, staged, target) = swap_fixture();
        let fail_install = |from: &Path, to: &Path| -> std::io::Result<()> {
            if from.ends_with(".horch.new") {
                Err(std::io::Error::other("antivirus lock"))
            } else {
                std::fs::rename(from, to)
            }
        };

        let err = swap_in(&staged, &target, true, fail_install).unwrap_err();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
        assert!(!old_path(&target).exists());
        let text = format!("{err:#}");
        assert!(text.contains("antivirus lock"), "{text}");
        assert!(text.contains("restored the previous"), "{text}");
    }

    #[test]
    fn swap_in_reports_both_errors_when_the_rollback_fails_too() {
        let (_tmp, staged, target) = swap_fixture();
        let old = old_path(&target);
        let fail_after_aside = |from: &Path, to: &Path| -> std::io::Result<()> {
            if from == old {
                Err(std::io::Error::other("rollback denied"))
            } else if from.ends_with(".horch.new") {
                Err(std::io::Error::other("antivirus lock"))
            } else {
                std::fs::rename(from, to)
            }
        };

        let err = swap_in(&staged, &target, true, fail_after_aside).unwrap_err();

        let text = format!("{err:#}");
        assert!(text.contains("antivirus lock"), "{text}");
        assert!(text.contains("rollback denied"), "{text}");
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "old");
    }

    #[test]
    fn old_path_is_a_dot_old_file_in_the_same_dir() {
        assert_eq!(
            old_path(Path::new("/d/horch.exe")),
            PathBuf::from("/d/.horch.exe.old")
        );
    }

    #[test]
    fn is_symlink_is_false_for_a_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_symlink(&tmp.path().join("missing")));
    }
}
