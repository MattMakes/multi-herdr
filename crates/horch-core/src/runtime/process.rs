//! Process helpers: PATH search, executable bits, PATH editing, a detached
//! spawn, and the child-only environment.
//!
//! Every function here is pure in its inputs. The PATH, PATHEXT and the
//! executable path come in as parameters, read once by the binary's bootstrap
//! into a [`RuntimeContext`](super::context::RuntimeContext).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

/// Executable extensions to try when a name has none: the `PATHEXT` entries
/// on Windows (lowercased), and nothing anywhere else.
pub(crate) fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    pathext
        .unwrap_or(".EXE;.CMD;.BAT;.COM")
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect()
}

/// Find `name` on a PATH value, honouring `PATHEXT` on Windows.
pub fn which(path: Option<&OsStr>, pathext: Option<&str>, name: &str) -> Option<PathBuf> {
    which_in(path?, name, &path_extensions(pathext))
}

/// Find `name` in an explicit PATH-formatted list. A directory is searched
/// whole, the bare name first and then each extension, before the next one.
pub(crate) fn which_in(paths: &OsStr, name: &str, extensions: &[String]) -> Option<PathBuf> {
    std::env::split_paths(paths)
        .filter(|d| !d.as_os_str().is_empty())
        .find_map(|dir| {
            let with_extensions = extensions
                .iter()
                .map(|ext| dir.join(format!("{name}{ext}")));
            std::iter::once(dir.join(name))
                .chain(with_extensions)
                .find(|candidate| is_executable(candidate))
        })
}

/// The binary a name or path stands for, when it exists: a path with a
/// separator must be a file, a bare name is searched on the context's PATH.
pub fn resolve(ctx: &super::context::RuntimeContext, bin: &Path) -> Option<PathBuf> {
    if bin.components().count() > 1 || bin.is_absolute() {
        return bin.is_file().then(|| bin.to_path_buf());
    }
    which(
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
        &bin.to_string_lossy(),
    )
}

/// SIGKILL the process group `pgid` (a child started with `process_group(0)`).
/// Whether the signal was sent; a group that is already gone gives ESRCH.
#[cfg(unix)]
pub fn kill_pgid(pgid: u32) -> bool {
    // SAFETY: killpg takes plain integers and touches no memory.
    unsafe { libc::killpg(pgid as libc::pid_t, libc::SIGKILL) == 0 }
}

#[cfg(not(unix))]
pub fn kill_pgid(_pgid: u32) -> bool {
    false
}

/// Kill the group of `child`, which was started with `process_group(0)`.
/// The child must not be reaped yet, so its pid still names its group. Off
/// unix there is no group, so only the child dies.
#[cfg(unix)]
pub fn kill_group(child: &mut Child) {
    kill_pgid(child.id());
}

#[cfg(not(unix))]
pub fn kill_group(child: &mut Child) {
    let _ = child.kill();
}

/// Poll `child` every `poll` until it exits or `deadline` passes. `Ok(None)`
/// means the deadline passed with the child still running; the child is left
/// alone, so the caller decides whether to kill it.
pub fn wait_until(
    child: &mut Child,
    deadline: Instant,
    poll: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(poll);
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Mark a freshly written file executable. No-op on Windows, where the
/// extension decides.
pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// A PATH with `dir` first, or `None` when it is already first.
pub(crate) fn path_with_prepended(current: &OsStr, dir: &Path) -> Option<OsString> {
    let existing: Vec<PathBuf> = std::env::split_paths(current)
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    if existing.first().map(|p| normalize(p)) == Some(normalize(dir)) {
        return None;
    }
    let target = normalize(dir);
    let mut entries = vec![dir.to_path_buf()];
    entries.extend(existing.into_iter().filter(|p| normalize(p) != target));
    std::env::join_paths(entries).ok()
}

/// The PATH a child of this binary needs: this binary's own directory first.
///
/// Every worker briefing tells the agent that `horch` is on its PATH, and the
/// agent invokes it by bare name from its own shell tool. The bash launchers made
/// that true with `export PATH="$root/bin:$PATH"`; without the equivalent, a
/// `horch` run from a build directory would leave every worker unable to reach the
/// orchestrator - its only channel. `None` when nothing needs to change.
pub(crate) fn path_with_own_dir(current_exe: &Path, path: Option<&OsStr>) -> Option<OsString> {
    let dir = current_exe.parent()?;
    path_with_prepended(path.unwrap_or_default(), dir)
}

/// Is `dir` present in an explicit PATH-formatted list?
pub fn on_path_in(paths: &OsStr, dir: &Path) -> bool {
    let target = normalize(dir);
    std::env::split_paths(paths).any(|p| normalize(&p) == target)
}

/// Compare paths the way the platform does: trailing separators are
/// insignificant, and Windows is case-insensitive.
fn normalize(p: &Path) -> OsString {
    let s = p.to_string_lossy();
    let trimmed = s.trim_end_matches(std::path::MAIN_SEPARATOR);
    let trimmed = if trimmed.is_empty() {
        s.as_ref()
    } else {
        trimmed
    };
    if cfg!(windows) {
        OsString::from(trimmed.to_ascii_lowercase())
    } else {
        OsString::from(trimmed)
    }
}

/// Remove every [`FORBIDDEN_ENV`](crate::harness::launch::FORBIDDEN_ENV) variable and
/// every git repository variable
/// ([`REPO_ENV`](horch_marketplace::git::REPO_ENV) and the numbered
/// `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`) from a child's environment.
///
/// A horch started under `git rebase -x` or a git hook has `GIT_DIR` set; an
/// agent or judge that inherited it would run its git in that repository.
/// Call it after any `env` call.
pub fn scrub_child_env(cmd: &mut Command) {
    for key in crate::harness::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    horch_marketplace::git::scrub_repo_env(cmd);
}

/// Give a child `vars`, except a key the command already sets or removes.
///
/// This is what inheriting them from the parent used to mean: a value the
/// builder put on the command itself always won over the parent's. A
/// forbidden key never gets through.
pub(crate) fn inherit_env<K, V>(cmd: &mut Command, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let taken: Vec<OsString> = cmd.get_envs().map(|(k, _)| k.to_os_string()).collect();
    for (key, value) in vars {
        let key = key.as_ref();
        if taken.iter().any(|t| t == key)
            || crate::harness::launch::FORBIDDEN_ENV
                .iter()
                .any(|f| OsStr::new(f) == key)
        {
            continue;
        }
        cmd.env(key, value);
    }
}

/// Spawn `cmd` so that it outlives this process's pane.
///
/// A new SESSION, not merely a new process group. Measured against herdr
/// 0.7.4: closing a pane tears down its whole session, so a child that only
/// left the process group is killed along with it, while one that called
/// `setsid` survives.
///
/// The child's environment is scrubbed here (`scrub_child_env`), right
/// before the spawn: no detached child (the judge job, the compaction job,
/// the grid tidy) can carry a forbidden key, whatever its caller set.
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<Child> {
    detach(cmd);
    cmd.spawn()
}

/// The pane identity of the process that starts a detached child (#218).
/// The child outlives that pane and serves the whole project, so it must not
/// act as that pane: it gets none of these unless its caller sets them.
pub const PANE_IDENTITY_ENV: &[&str] = &[
    "HORCH_ROLE",
    "HORCH_RECORD_ID",
    "HORCH_WORKER_ID",
    "HORCH_WORKSPACE_ID",
    "HERDR_PANE_ID",
];

/// Remove from `cmd` the [`PANE_IDENTITY_ENV`] it would inherit from
/// `inherited`, and a `TMPDIR` under `<state_root>/tmp/`: a pane's private
/// temp dir, which the pane removes when it ends. A key the caller set or
/// removed on `cmd` itself is left as the caller wants it.
fn scrub_pane_identity(cmd: &mut Command, inherited: &dyn super::context::EnvSource) {
    let taken: Vec<OsString> = cmd.get_envs().map(|(k, _)| k.to_os_string()).collect();
    let is_taken = |key: &str| taken.iter().any(|t| t == key);
    for key in PANE_IDENTITY_ENV {
        if !is_taken(key) {
            cmd.env_remove(key);
        }
    }
    if is_taken("TMPDIR") {
        return;
    }
    let home = super::paths::home_dir(inherited);
    let mut roots = vec![super::paths::state_root(inherited, &home)];
    if let Some((_, Some(dir))) = cmd.get_envs().find(|(k, _)| *k == "HORCH_STATE_DIR") {
        roots.push(PathBuf::from(dir));
    }
    let pane_tmp = inherited
        .var_os("TMPDIR")
        .map(PathBuf::from)
        .is_some_and(|t| roots.iter().any(|r| t.starts_with(r.join("tmp"))));
    if pane_tmp {
        cmd.env_remove("TMPDIR");
    }
}

/// What [`spawn_detached`] does to `cmd` before the spawn.
fn detach(cmd: &mut Command) {
    scrub_child_env(cmd);
    scrub_pane_identity(cmd, &super::context::ProcessEnv);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: runs in the forked child between fork and exec. `setsid` is
        // async-signal-safe and touches no state this process shares.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::context::MapEnv;

    #[cfg(unix)]
    fn group_leader(script: &str) -> Child {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        cmd.spawn().unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn wait_until_reports_an_exit_and_leaves_a_runner_alone() {
        let mut quick = group_leader("exit 3");
        let far = Instant::now() + Duration::from_secs(60);
        let status = wait_until(&mut quick, far, Duration::from_millis(5)).unwrap();
        assert_eq!(status.and_then(|s| s.code()), Some(3));

        let mut slow = group_leader("sleep 60");
        let soon = Instant::now() + Duration::from_millis(50);
        assert!(wait_until(&mut slow, soon, Duration::from_millis(5))
            .unwrap()
            .is_none());
        assert!(
            slow.try_wait().unwrap().is_none(),
            "the child is not killed"
        );
        kill_group(&mut slow);
        slow.wait().unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn kill_group_kills_the_grandchildren_too() {
        // `sh` backgrounds a sleeper that prints its pid, then waits on it.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 60 & echo $!; wait"]);
        cmd.stdout(std::process::Stdio::piped());
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(child.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        let grandchild: libc::pid_t = line.trim().parse().unwrap();
        kill_group(&mut child);
        child.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        // SAFETY: signal 0 only tests that the pid exists.
        while unsafe { libc::kill(grandchild, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(
            // SAFETY: signal 0 only tests that the pid exists.
            unsafe { libc::kill(grandchild, 0) },
            0,
            "grandchild still runs"
        );
    }

    #[test]
    #[cfg(unix)]
    fn kill_pgid_is_false_for_a_group_that_is_gone() {
        let mut child = group_leader("exit 0");
        let pgid = child.id();
        child.wait().unwrap();
        assert!(!kill_pgid(pgid));
    }

    #[test]
    fn resolve_takes_a_path_as_is_and_searches_a_bare_name() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("tool");
        std::fs::write(&bin, "").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let env = MapEnv::new(dir.path())
            .with("HOME", &dir.path().to_string_lossy())
            .with("PATH", &dir.path().to_string_lossy());
        let ctx = crate::runtime::RuntimeContext::from_env(&env).unwrap();
        assert_eq!(resolve(&ctx, &bin), Some(bin.clone()));
        assert_eq!(resolve(&ctx, &dir.path().join("missing")), None);
        assert_eq!(resolve(&ctx, Path::new("tool")), Some(bin));
        assert_eq!(resolve(&ctx, Path::new("absent")), None);
    }

    /// A detached child never carries a forbidden key: `spawn_detached`
    /// removes every one itself, after the caller's `env` calls.
    #[test]
    fn spawn_detached_scrubs_every_forbidden_key() {
        let mut cmd = Command::new("true");
        cmd.env("HORCH_TEST_KEEP", "1");
        detach(&mut cmd);
        let envs: Vec<(OsString, Option<OsString>)> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(OsStr::to_os_string)))
            .collect();
        for key in crate::harness::launch::FORBIDDEN_ENV {
            assert!(
                envs.iter().any(|(k, v)| k == key && v.is_none()),
                "{key} is not removed"
            );
        }
        assert!(envs
            .iter()
            .any(|(k, v)| k == "HORCH_TEST_KEEP" && v.as_deref() == Some(OsStr::new("1"))));
    }

    /// The env `cmd` gives its child, as the caller and the scrub left it:
    /// `None` is a removed key.
    fn envs_of(cmd: &Command) -> Vec<(String, Option<String>)> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    /// #218 through a real spawn: this test binary runs again as a process
    /// of a worker pane, and its `spawn_detached` child prints its env.
    #[cfg(unix)]
    #[test]
    fn spawn_detached_gives_its_child_none_of_the_panes_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let pane_tmp = state.join("tmp").join("sonnet-2");
        std::fs::create_dir_all(&pane_tmp).unwrap();
        let seen = tmp.path().join("env.txt");
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::process::tests::detached_identity_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HORCH_TEST_DETACHED_ENV", &seen)
            .env("HORCH_STATE_DIR", &state)
            .env("TMPDIR", &pane_tmp)
            .env("HORCH_ROLE", "sonnet-2")
            .env("HORCH_RECORD_ID", "r-1")
            .env("HORCH_WORKER_ID", "w-123")
            .env("HORCH_WORKSPACE_ID", "W2")
            .env("HERDR_PANE_ID", "p_9")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let env = std::fs::read_to_string(&seen).unwrap();
        let keys: Vec<&str> = env.lines().filter_map(|l| l.split('=').next()).collect();
        for key in PANE_IDENTITY_ENV.iter().chain(&["TMPDIR"]) {
            assert!(!keys.contains(key), "{key} reached the child:\n{env}");
        }
        assert!(
            keys.contains(&"HORCH_STATE_DIR"),
            "the rest of the env stays:\n{env}"
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "run by spawn_detached_gives_its_child_none_of_the_panes_identity, in a pane's env"]
    fn detached_identity_child() {
        let seen = std::env::var_os("HORCH_TEST_DETACHED_ENV").expect("run by its parent test");
        let file = std::fs::File::create(seen).unwrap();
        let mut cmd = Command::new("/usr/bin/env");
        cmd.stdout(file);
        assert!(spawn_detached(&mut cmd).unwrap().wait().unwrap().success());
    }

    /// #218: a child started from a worker pane does not carry that pane's
    /// identity or its private TMPDIR.
    #[test]
    fn a_detached_child_drops_the_starting_panes_identity() {
        let mut inherited = MapEnv::new("/cwd");
        inherited = inherited.with("HOME", "/home/a");
        inherited = inherited.with("HORCH_STATE_DIR", "/state");
        inherited = inherited.with("HORCH_ROLE", "sonnet-2");
        inherited = inherited.with("HORCH_WORKER_ID", "w-123");
        inherited = inherited.with("HORCH_WORKSPACE_ID", "W2");
        inherited = inherited.with("TMPDIR", "/state/tmp/sonnet-2");
        let mut cmd = Command::new("true");
        scrub_pane_identity(&mut cmd, &inherited);
        let envs = envs_of(&cmd);
        for key in PANE_IDENTITY_ENV.iter().chain(&["TMPDIR"]) {
            assert!(
                envs.contains(&(key.to_string(), None)),
                "{key} is not removed: {envs:?}"
            );
        }
    }

    /// What the caller sets on purpose stays: the watchdog's run workspace,
    /// the Prime stop's TMPDIR. An operator's own TMPDIR is not a pane's.
    #[test]
    fn a_detached_child_keeps_what_its_caller_sets_and_a_plain_tmpdir() {
        let mut inherited = MapEnv::new("/cwd");
        inherited = inherited.with("HOME", "/home/a");
        inherited = inherited.with("HORCH_WORKSPACE_ID", "W2");
        inherited = inherited.with("TMPDIR", "/var/folders/xy/T/");
        let mut cmd = Command::new("true");
        cmd.env("HORCH_WORKSPACE_ID", "W1");
        scrub_pane_identity(&mut cmd, &inherited);
        let envs = envs_of(&cmd);
        assert!(envs.contains(&("HORCH_WORKSPACE_ID".into(), Some("W1".into()))));
        assert!(!envs.iter().any(|(k, _)| k == "TMPDIR"), "{envs:?}");

        // The default state root, and a state dir the caller passes.
        for (state_dir, tmp) in [
            (None, "/home/a/.local/state/horch/tmp/codex-1"),
            (Some("/elsewhere"), "/elsewhere/tmp/codex-1"),
        ] {
            let mut inherited = MapEnv::new("/cwd");
            inherited = inherited.with("HOME", "/home/a");
            inherited = inherited.with("TMPDIR", tmp);
            let mut cmd = Command::new("true");
            if let Some(dir) = state_dir {
                cmd.env("HORCH_STATE_DIR", dir);
            }
            scrub_pane_identity(&mut cmd, &inherited);
            assert!(envs_of(&cmd).contains(&("TMPDIR".into(), None)), "{tmp}");
        }
        let mut cmd = Command::new("true");
        cmd.env("TMPDIR", "/state/tmp/prime");
        scrub_pane_identity(&mut cmd, &inherited);
        assert!(envs_of(&cmd).contains(&("TMPDIR".into(), Some("/state/tmp/prime".into()))));
    }

    fn path_list(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn which_in_finds_only_executables() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plainfile");
        std::fs::write(&plain, "").unwrap();
        let exe = tmp.path().join("runnable");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        make_executable(&exe).unwrap();

        let paths = path_list(&[tmp.path()]);
        assert_eq!(which_in(&paths, "runnable", &[]), Some(exe.clone()));
        assert_eq!(which_in(&paths, "plainfile", &[]), None);
        assert_eq!(which_in(&paths, "absent", &[]), None);
        assert_eq!(which(Some(&paths), None, "runnable"), Some(exe));
        assert_eq!(which(None, None, "runnable"), None, "no PATH, no match");
    }

    /// On Windows the bare name has no extension, so PATHEXT must be tried.
    #[test]
    fn which_in_tries_path_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("tool.exe");
        std::fs::write(&exe, "").unwrap();
        make_executable(&exe).unwrap();

        let paths = path_list(&[tmp.path()]);
        assert_eq!(which_in(&paths, "tool", &[]), None, "no extensions tried");
        assert_eq!(
            which_in(&paths, "tool", &[".exe".to_string()]),
            Some(exe),
            "PATHEXT entry should match"
        );
    }

    #[test]
    fn path_extensions_apply_only_on_windows() {
        if cfg!(windows) {
            assert_eq!(path_extensions(Some(".EXE;;.Cmd")), vec![".exe", ".cmd"]);
            assert_eq!(path_extensions(None).len(), 4);
        } else {
            assert!(path_extensions(Some(".EXE")).is_empty());
        }
    }

    /// Earlier PATH entries win.
    #[test]
    fn which_in_respects_path_order() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for dir in [first.path(), second.path()] {
            let exe = dir.join("dup");
            std::fs::write(&exe, "").unwrap();
            make_executable(&exe).unwrap();
        }
        let paths = path_list(&[first.path(), second.path()]);
        assert_eq!(which_in(&paths, "dup", &[]), Some(first.path().join("dup")));
    }

    /// In one directory the bare name beats an extension, and the extensions
    /// go in the order given.
    #[test]
    fn which_in_tries_the_bare_name_then_each_extension_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        for file in ["tool", "tool.cmd", "tool.exe", "only.cmd", "only.exe"] {
            let exe = tmp.path().join(file);
            std::fs::write(&exe, "").unwrap();
            make_executable(&exe).unwrap();
        }
        let paths = path_list(&[tmp.path()]);
        let extensions = [".exe".to_string(), ".cmd".to_string()];
        assert_eq!(
            which_in(&paths, "tool", &extensions),
            Some(tmp.path().join("tool"))
        );
        assert_eq!(
            which_in(&paths, "only", &extensions),
            Some(tmp.path().join("only.exe"))
        );
    }

    /// A directory is searched whole before the next one: an extension in an
    /// earlier directory beats the bare name in a later one.
    #[test]
    fn which_in_finishes_a_directory_before_the_next() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for exe in [first.path().join("tool.exe"), second.path().join("tool")] {
            std::fs::write(&exe, "").unwrap();
            make_executable(&exe).unwrap();
        }
        let paths = path_list(&[first.path(), second.path()]);
        assert_eq!(
            which_in(&paths, "tool", &[".exe".to_string()]),
            Some(first.path().join("tool.exe"))
        );
    }

    /// A file without the executable bit stops nothing: the extension in the
    /// same directory and the bare name in a later one still match.
    #[cfg(unix)]
    #[test]
    fn which_in_passes_over_files_without_the_executable_bit() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("tool"), "").unwrap();
        std::fs::write(first.path().join("other"), "").unwrap();
        for exe in [first.path().join("tool.exe"), second.path().join("other")] {
            std::fs::write(&exe, "").unwrap();
            make_executable(&exe).unwrap();
        }
        let paths = path_list(&[first.path(), second.path()]);
        let extensions = [".exe".to_string()];
        assert_eq!(
            which_in(&paths, "tool", &extensions),
            Some(first.path().join("tool.exe"))
        );
        assert_eq!(
            which_in(&paths, "other", &extensions),
            Some(second.path().join("other"))
        );
    }

    #[test]
    fn on_path_in_ignores_trailing_separators() {
        let tmp = tempfile::tempdir().unwrap();
        let with_sep = OsString::from(format!(
            "{}{}",
            tmp.path().display(),
            std::path::MAIN_SEPARATOR
        ));
        assert!(on_path_in(&with_sep, tmp.path()));
        assert!(!on_path_in(&OsString::from("/nowhere"), tmp.path()));
    }

    #[test]
    fn on_path_in_handles_an_empty_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!on_path_in(&OsString::new(), tmp.path()));
    }

    /// Workers invoke `horch` by bare name, so its own directory must come first.
    #[test]
    fn path_with_prepended_puts_the_directory_first() {
        let dir = Path::new("/opt/horch/bin");
        let next = path_with_prepended(&path_list(&[Path::new("/usr/bin")]), dir).unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(&next).collect();
        assert_eq!(entries, vec![dir.to_path_buf(), PathBuf::from("/usr/bin")]);
    }

    #[test]
    fn path_with_prepended_is_a_noop_when_already_first() {
        let dir = Path::new("/opt/horch/bin");
        let current = path_list(&[dir, Path::new("/usr/bin")]);
        assert_eq!(path_with_prepended(&current, dir), None);
    }

    /// An entry further down is moved to the front, not duplicated, so repeated
    /// nesting cannot grow PATH without bound.
    #[test]
    fn path_with_prepended_moves_rather_than_duplicates() {
        let dir = Path::new("/opt/horch/bin");
        let current = path_list(&[Path::new("/usr/bin"), dir]);
        let next = path_with_prepended(&current, dir).unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(&next).collect();
        assert_eq!(entries, vec![dir.to_path_buf(), PathBuf::from("/usr/bin")]);
    }

    #[test]
    fn path_with_prepended_handles_an_empty_path() {
        let dir = Path::new("/opt/horch/bin");
        let next = path_with_prepended(&OsString::new(), dir).unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(&next).collect();
        assert_eq!(entries, vec![dir.to_path_buf()]);
    }

    /// The child PATH must make the running binary's directory reachable, and
    /// the test process's own PATH stays as it was.
    #[test]
    fn path_with_own_dir_makes_this_binary_findable() {
        let exe = Path::new("/opt/horch/bin/horch");
        let next = path_with_own_dir(exe, Some(OsStr::new("/nonexistent-for-test"))).unwrap();
        assert!(on_path_in(&next, Path::new("/opt/horch/bin")), "{next:?}");
        let next = path_with_own_dir(exe, None).unwrap();
        assert!(on_path_in(&next, Path::new("/opt/horch/bin")), "{next:?}");
    }

    #[test]
    fn inherit_env_keeps_what_the_command_already_sets() {
        let mut cmd = Command::new("x");
        cmd.env("A", "builder").env_remove("B");
        inherit_env(
            &mut cmd,
            [
                ("A", "parent"),
                ("B", "parent"),
                ("C", "parent"),
                ("ANTHROPIC_API_KEY", "nope"),
            ],
        );
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(
            envs,
            vec![
                ("A".into(), Some("builder".into())),
                ("B".into(), None),
                ("C".into(), Some("parent".into())),
            ]
        );
    }
}
