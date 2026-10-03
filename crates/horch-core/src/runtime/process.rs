//! Process helpers: PATH search, executable bits, PATH editing, a detached
//! spawn, and the child-only environment.
//!
//! Every function here is pure in its inputs. The PATH, PATHEXT and the
//! executable path come in as parameters, read once by the binary's bootstrap
//! into a [`RuntimeContext`](super::context::RuntimeContext).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Executable extensions to try when a name has none: the `PATHEXT` entries
/// on Windows (lowercased), and nothing anywhere else.
pub fn path_extensions(pathext: Option<&str>) -> Vec<String> {
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

/// Find `name` in an explicit PATH-formatted list.
pub fn which_in(paths: &OsStr, name: &str, extensions: &[String]) -> Option<PathBuf> {
    for dir in std::env::split_paths(paths).filter(|d| !d.as_os_str().is_empty()) {
        let direct = dir.join(name);
        if is_executable(&direct) {
            return Some(direct);
        }
        for ext in extensions {
            let with_ext = dir.join(format!("{name}{ext}"));
            if is_executable(&with_ext) {
                return Some(with_ext);
            }
        }
    }
    None
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
pub fn path_with_prepended(current: &OsStr, dir: &Path) -> Option<OsString> {
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
pub fn path_with_own_dir(current_exe: &Path, path: Option<&OsStr>) -> Option<OsString> {
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

/// Remove every [`FORBIDDEN_ENV`](crate::launch::FORBIDDEN_ENV) variable from
/// a child's environment.
pub fn strip_forbidden(cmd: &mut Command) {
    for key in crate::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
}

/// Give a child `vars`, except a key the command already sets or removes.
///
/// This is what inheriting them from the parent used to mean: a value the
/// builder put on the command itself always won over the parent's. A
/// forbidden key never gets through.
pub fn inherit_env<K, V>(cmd: &mut Command, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let taken: Vec<OsString> = cmd.get_envs().map(|(k, _)| k.to_os_string()).collect();
    for (key, value) in vars {
        let key = key.as_ref();
        if taken.iter().any(|t| t == key)
            || crate::launch::FORBIDDEN_ENV
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
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<Child> {
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
    cmd.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

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
