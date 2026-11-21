use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_core::runtime::{process, RuntimeContext};
use horch_platform::service;

use super::fleetd::{self, Fleet, Mode};

pub fn default_dir(ctx: &RuntimeContext) -> PathBuf {
    match horch_platform::path::INSTALL_BASE_VAR {
        Some(_) => ctx
            .inherited
            .local_app_data
            .as_ref()
            .map(|base| Path::new(base).join("Programs").join("horch"))
            .unwrap_or_else(|| ctx.paths.home.join("horch")),
        None => ctx.paths.home.join(".local").join("bin"),
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

    place_horch(&source, &dir, &target)?;
    let herdr = horch_core::workspace::PaneHost::of(ctx);
    if let Err(e) = super::telemetry::restart_if_stale(ctx, &herdr, &target) {
        println!("warning: telemetry collector not restarted: {e:#}; run `horch telemetry ensure`");
    }
    let install_fleetd_service = || install_service(&target);
    for line in fleetd::with_fleet(ctx, |fleet| {
        install_fleetd(fleet, &install_fleetd_service, &target)
    }) {
        println!("{line}");
    }

    install_siblings(&source, &dir)?;
    let git = horch_core::taskledger::git::Git::new(&ctx.bins.harness.git);
    let roster = ctx
        .bins
        .roster_override
        .as_deref()
        .map(|dir| durable_roster(&git, dir));
    install_launchers(&dir, roster.as_deref())?;

    let on_path = ctx
        .inherited
        .path
        .as_ref()
        .is_some_and(|path| process::on_path_in(path, &dir));
    println!("{}", path_help(&dir, on_path));

    println!("\nInstalled version: {}", installed_version(&target)?);
    Ok(())
}

fn install_fleetd(
    fleet: &Fleet<'_>,
    install_service: &dyn Fn() -> Result<()>,
    target: &Path,
) -> Vec<String> {
    let mut lines = Vec::new();
    if fleet.mode == Mode::Service {
        if let Err(e) = install_service() {
            lines.push(format!(
                "warning: the fleetd service is not installed: {e:#}"
            ));
        }
    }
    match fleetd::restart_older(fleet, target) {
        Ok(Some(checked)) => lines.push(fleetd::describe(&checked)),
        Ok(None) => {}
        Err(e) => lines.push(format!(
            "warning: fleetd was not restarted on {}: {e:#}; run `horch fleetd check`",
            target.display()
        )),
    }
    lines
}

fn install_service(target: &Path) -> Result<()> {
    let spec = fleetd::service_spec(target)?;
    service::ensure(&spec, Path::new(service::MANAGER))
        .with_context(|| format!("installing the service {}", service::LABEL))
}

fn place_horch(source: &Path, dir: &Path, target: &Path) -> Result<()> {
    remove_stale_old(dir);
    remove_retired(dir)?;
    install_one("horch", source, target)
}

fn remove_retired(dir: &Path) -> Result<()> {
    for path in horch::remove_retired_binaries(dir)? {
        println!("removed the retired {}", path.display());
    }
    Ok(())
}

fn remove_stale_old(dir: &Path) {
    if horch_platform::rest::RUNNING_EXE_IS_LOCKED {
        for old in stale_old_paths(dir) {
            let _ = std::fs::remove_file(old);
        }
    }
}

fn path_help(dir: &Path, on_path: bool) -> String {
    if on_path {
        "\nReady. cd into a project and run: horch fleet\n(needs a running herdr server: launch the herdr app, or `herdr server`)".to_string()
    } else {
        format!(
            "\n{} is not on your PATH. Add it:\n\n{}\n\nThen open a new terminal, cd into a project, and run: horch fleet",
            dir.display(),
            horch_platform::rest::path_help(dir)
        )
    }
}

const FLEET_LAUNCHER: &str = include_str!("../../../../scripts/herdr-fleet");
const RUN_LAUNCHER: &str = include_str!("../../../../scripts/herdr-run");

pub fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn install_siblings(horch_source: &Path, dir: &Path) -> Result<()> {
    for &bin in &horch::RELEASE_BINARIES[1..] {
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

fn install_launchers(dir: &Path, teammates: Option<&Path>) -> Result<()> {
    if !horch_platform::rest::SHELL_LAUNCHERS {
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

fn durable_roster(git: &horch_core::taskledger::git::Git, teammates: &Path) -> PathBuf {
    let kept = teammates.to_path_buf();
    let Ok(top) = git.run(teammates, &["rev-parse", "--show-toplevel"]) else {
        return kept;
    };
    let top = PathBuf::from(top);
    let top = top.canonicalize().unwrap_or(top);
    let Some(main) = git.main_root(teammates) else {
        return kept;
    };
    if top == main {
        return kept;
    }
    let dir = teammates.canonicalize().unwrap_or(kept.clone());
    let Ok(rel) = dir.strip_prefix(&top) else {
        return kept;
    };
    let durable = main.join(rel);
    if !durable.is_dir() {
        return kept;
    }
    println!(
        "{} is in a linked worktree, which is deleted when its work lands; \
         the launchers use {} instead",
        teammates.display(),
        durable.display()
    );
    durable
}

const TEAMMATES_LINE: &str = "HORCH_TEAMMATES_DIR=\"__TEAMMATES_DIR__\"";

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

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

fn sibling(horch_source: &Path, name: &str) -> PathBuf {
    let real = horch_source
        .canonicalize()
        .unwrap_or_else(|_| horch_source.to_path_buf());
    real.with_file_name(exe_name(name))
}

fn install_one(name: &str, source: &Path, target: &Path) -> Result<()> {
    if same_file(source, target) && !is_symlink(target) {
        println!("{name} is already installed at {}", target.display());
    } else {
        place_binary(source, target)?;
        println!("installed {name} to {}", target.display());
    }
    Ok(())
}

fn place_binary(source: &Path, target: &Path) -> Result<()> {
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    replace_file(target, |staged| {
        std::fs::copy(&source, staged)
            .with_context(|| format!("copying {} to {}", source.display(), staged.display()))?;
        Ok(())
    })
}

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
    swap_in(
        &staged,
        target,
        horch_platform::rest::RUNNING_EXE_IS_LOCKED,
        |from, to| std::fs::rename(from, to),
    )
}

fn swap_in(
    staged: &Path,
    target: &Path,
    move_aside: bool,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let aside = move_aside_old(target, move_aside, &rename)?;
    rename(staged, target).map_err(|e| roll_back(install_error(e, target), aside, target, &rename))
}

fn move_aside_old(
    target: &Path,
    enabled: bool,
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<PathBuf>> {
    if !(enabled && target.exists()) {
        return Ok(None);
    }
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    rename(target, &old)
        .with_context(|| format!("moving {} aside to {}", target.display(), old.display()))?;
    Ok(Some(old))
}

fn install_error(e: std::io::Error, target: &Path) -> anyhow::Error {
    anyhow::Error::new(e).context(format!(
        "installing to {} (is a horch process running from there?)",
        target.display()
    ))
}

fn roll_back(
    err: anyhow::Error,
    aside: Option<PathBuf>,
    target: &Path,
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> anyhow::Error {
    let Some(old) = aside else {
        return err;
    };
    match rename(&old, target) {
        Ok(()) => err.context(format!("restored the previous {}", target.display())),
        Err(rollback) => err.context(format!(
            "restoring {} from {} also failed: {rollback}",
            target.display(),
            old.display()
        )),
    }
}

fn stale_old_paths(dir: &Path) -> Vec<PathBuf> {
    horch::RELEASE_BINARIES
        .iter()
        .map(|bin| old_path(&dir.join(exe_name(bin))))
        .collect()
}

fn old_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.old"))
}

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

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

fn installed_version(target: &Path) -> Result<String> {
    let out = horch_platform::start::command(target)
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

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch::HERDR_INSTALL_BIN;

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
            sibling(&link, HERDR_INSTALL_BIN),
            build.canonicalize().unwrap().join(HERDR_INSTALL_BIN)
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
    fn trf_47_place_horch_removes_old_dataset_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let horch = build_dir(tmp.path(), &["horch"]);
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let dataset = bin.join(exe_name(horch::RETIRED_BINARIES[0]));
        std::fs::write(&dataset, "old").unwrap();
        let target = bin.join(exe_name("horch"));

        place_horch(&horch, &bin, &target).unwrap();

        assert!(!dataset.exists());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "horch");
    }

    #[test]
    fn trf_47_install_siblings_installs_only_herdr_install() {
        let tmp = tempfile::tempdir().unwrap();
        let horch = build_dir(tmp.path(), &["horch", "old-sibling", HERDR_INSTALL_BIN]);
        let bin = tmp.path().join("bin");

        install_siblings(&horch, &bin).unwrap();

        let mut names: Vec<_> = std::fs::read_dir(&bin)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec![exe_name(HERDR_INSTALL_BIN)]);
        assert_eq!(
            std::fs::read_to_string(bin.join(exe_name(HERDR_INSTALL_BIN))).unwrap(),
            HERDR_INSTALL_BIN
        );
    }

    #[test]
    fn install_siblings_goes_on_without_herdr_install() {
        let tmp = tempfile::tempdir().unwrap();
        let horch = build_dir(tmp.path(), &["horch"]);
        let bin = tmp.path().join("bin");

        install_siblings(&horch, &bin).unwrap();

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
    fn a_roster_in_a_linked_worktree_maps_to_the_main_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {out:?}");
        };
        std::fs::create_dir_all(main.join("teammates")).unwrap();
        std::fs::write(main.join("teammates/a.md"), "a").unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["add", "."]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "init",
            ],
        );
        let gate = main.join(".worktrees/_gate");
        git(
            &main,
            &["worktree", "add", "-q", "--detach", gate.to_str().unwrap()],
        );
        let g = horch_core::taskledger::git::Git::new("git");
        let main_roster = main.canonicalize().unwrap().join("teammates");

        assert_eq!(durable_roster(&g, &gate.join("teammates")), main_roster);
        assert_eq!(
            durable_roster(&g, &main.join("teammates")),
            main.join("teammates")
        );
        let outside = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        assert_eq!(durable_roster(&g, &outside), outside);
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
    fn trf_47_stale_old_paths_name_the_2_binaries() {
        assert_eq!(
            stale_old_paths(Path::new("/d")),
            vec![
                PathBuf::from(format!("/d/.horch{}.old", std::env::consts::EXE_SUFFIX)),
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
    fn swap_in_without_move_aside_reports_the_failed_install_and_keeps_the_target() {
        let (_tmp, staged, target) = swap_fixture();
        let fail = |_: &Path, _: &Path| -> std::io::Result<()> {
            Err(std::io::Error::other("sharing violation"))
        };

        let err = swap_in(&staged, &target, false, fail).unwrap_err();

        let text = format!("{err:#}");
        assert!(text.contains("installing to"), "{text}");
        assert!(text.contains("sharing violation"), "{text}");
        assert!(!text.contains("restored"), "{text}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
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

    #[test]
    fn path_help_on_path_points_at_fleet() {
        let help = path_help(Path::new("/x/bin"), true);
        assert!(help.starts_with("\nReady. cd into a project and run: horch fleet\n"));
        assert!(!help.contains("not on your PATH"));
    }

    #[test]
    fn path_help_off_path_shows_the_fix_for_this_platform() {
        let help = path_help(Path::new("/x/bin"), false);
        assert!(help.starts_with("\n/x/bin is not on your PATH. Add it:\n\n"));
        assert!(
            help.ends_with("\n\nThen open a new terminal, cd into a project, and run: horch fleet")
        );
        if cfg!(windows) {
            assert!(help.contains("setx PATH \"/x/bin;"));
        } else {
            assert!(help.contains("export PATH=\"/x/bin:$PATH\""));
        }
    }

    #[test]
    fn install_writes_no_service_in_stand_in_mode() {
        use crate::cmd::fleetd::{test_fleet::World, Mode};
        let w = World::new();
        let target = w.new_binary("bin");
        let installs = std::cell::Cell::new(0);
        let service = || -> Result<()> {
            installs.set(installs.get() + 1);
            Ok(())
        };
        let (lines, starts) = w.run(Mode::StandIn, |f| install_fleetd(f, &service, &target));
        assert_eq!(installs.get(), 0, "stand-in mode installed a service");
        assert!(lines.is_empty(), "{lines:?}");
        assert!(starts.is_empty(), "{starts:?}");
        let (lines, starts) = w.run(Mode::Service, |f| install_fleetd(f, &service, &target));
        assert_eq!(installs.get(), 1, "service mode installs the service");
        assert!(lines.is_empty(), "{lines:?}");
        assert!(
            starts.is_empty(),
            "no fleetd ran, so none restarts: {starts:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_restarts_a_fleetd_of_another_binary() {
        use crate::cmd::fleetd::test_fleet::{alive, ended, World};
        use crate::cmd::fleetd::Mode;
        let w = World::new();
        let old = w.process();
        w.record(old);
        w.heartbeat(old, 1);
        let no_service = || -> Result<()> { panic!("stand-in mode installs no service") };
        let (lines, starts) = w.run(Mode::StandIn, |f| install_fleetd(f, &no_service, &w.exe));
        assert!(lines.is_empty(), "{lines:?}");
        assert!(starts.is_empty(), "{starts:?}");
        assert!(alive(old), "a fleetd of the installed binary was stopped");
        let target = w.new_binary("bin");
        let (lines, starts) = w.run(Mode::StandIn, |f| install_fleetd(f, &no_service, &target));
        assert!(ended(old), "the fleetd of the other binary still runs");
        assert_eq!(starts, [target], "the new fleetd runs the installed binary");
        let new = w.row().expect("a fleetd row");
        assert_ne!(new, old);
        assert!(alive(new));
        assert_eq!(
            lines,
            [format!(
                "fleetd PID {} runs an older binary. It was stopped, and fleetd PID {} runs now.",
                old.pid, new.pid
            )]
        );
    }
}
