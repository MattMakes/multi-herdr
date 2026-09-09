//! Locating the agent CLIs a worker pane launches.
//!
//! The PATH search is split into a pure `*_in` function plus a thin env-reading
//! wrapper, so it can be tested without mutating process-wide environment.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Which Claude CLI to launch.
///
/// The bash implementation hard-coded a sibling checkout
/// (`../claude-code-proxy/bin/cpx`), which cannot exist on a fresh Windows
/// machine. Resolution order now:
///   1. `$HORCH_CLAUDE_BIN` - explicit override.
///   2. `cpx` on PATH - preserves the proxy setup where it is already installed.
///   3. `claude` - the plain Claude Code CLI.
pub fn claude_bin() -> PathBuf {
    if let Some(explicit) = env_path("HORCH_CLAUDE_BIN") {
        return explicit;
    }
    if which("cpx").is_some() {
        return PathBuf::from("cpx");
    }
    PathBuf::from("claude")
}

/// Which Codex CLI to launch. `$HORCH_CODEX_BIN` overrides.
pub fn codex_bin() -> PathBuf {
    env_path("HORCH_CODEX_BIN").unwrap_or_else(|| PathBuf::from("codex"))
}

/// Which OpenCode CLI to launch. `$HORCH_OPENCODE_BIN` overrides.
pub fn opencode_bin() -> PathBuf {
    env_path("HORCH_OPENCODE_BIN").unwrap_or_else(|| PathBuf::from("opencode"))
}

/// Which pi CLI to launch. `$HORCH_PI_BIN` overrides.
pub fn pi_bin() -> PathBuf {
    env_path("HORCH_PI_BIN").unwrap_or_else(|| PathBuf::from("pi"))
}

/// Which Prime Agent CLI to launch. `$HORCH_PRIME_BIN` overrides.
pub fn prime_bin() -> PathBuf {
    env_path("HORCH_PRIME_BIN").unwrap_or_else(|| PathBuf::from("prime-agent"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Executable extensions to try when a name has none. Empty off Windows.
fn path_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect()
}

/// Find `name` on PATH, honouring `PATHEXT` on Windows.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(&std::env::var_os("PATH")?, name, &path_extensions())
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

/// Put this binary's own directory first on PATH, for the benefit of children.
///
/// Every worker briefing tells the agent that `horch` is on its PATH, and the
/// agent invokes it by bare name from its own shell tool. The bash launchers made
/// that true with `export PATH="$root/bin:$PATH"`; without the equivalent, a
/// `horch` run from a build directory would leave every worker unable to reach the
/// orchestrator - its only channel.
pub fn prepend_own_dir_to_path() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let Some(dir) = exe.parent() else {
        return Ok(());
    };
    let current = std::env::var_os("PATH").unwrap_or_default();
    if let Some(next) = path_with_prepended(&current, dir) {
        std::env::set_var("PATH", next);
    }
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

/// Is `dir` already on PATH?
pub fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH")
        .map(|paths| on_path_in(&paths, dir))
        .unwrap_or(false)
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
    let trimmed = if trimmed.is_empty() { s.as_ref() } else { trimmed };
    if cfg!(windows) {
        OsString::from(trimmed.to_ascii_lowercase())
    } else {
        OsString::from(trimmed)
    }
}

/// The user's home directory.
pub fn home_dir() -> PathBuf {
    let key: &OsStr = if cfg!(windows) {
        OsStr::new("USERPROFILE")
    } else {
        OsStr::new("HOME")
    };
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Guards the few tests that must mutate process-wide environment, which is
    /// shared across Rust's parallel test threads.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env(key: &str, value: Option<&str>, f: impl FnOnce()) {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(key);
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    fn path_list(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn explicit_override_wins() {
        with_env("HORCH_CLAUDE_BIN", Some("/custom/agent"), || {
            assert_eq!(claude_bin(), PathBuf::from("/custom/agent"));
        });
        with_env("HORCH_CODEX_BIN", Some("/custom/codex"), || {
            assert_eq!(codex_bin(), PathBuf::from("/custom/codex"));
        });
    }

    /// An empty override is treated as unset, not as an empty command.
    #[test]
    fn empty_override_is_ignored() {
        with_env("HORCH_CODEX_BIN", Some(""), || {
            assert_eq!(codex_bin(), PathBuf::from("codex"));
        });
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
        assert_eq!(which_in(&paths, "runnable", &[]), Some(exe));
        assert_eq!(which_in(&paths, "plainfile", &[]), None);
        assert_eq!(which_in(&paths, "absent", &[]), None);
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

    /// The real call must make the running binary's directory reachable.
    #[test]
    fn prepend_own_dir_to_path_makes_this_binary_findable() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("PATH");
        std::env::set_var("PATH", "/nonexistent-for-test");

        prepend_own_dir_to_path().unwrap();
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let path = std::env::var_os("PATH").unwrap();
        assert!(on_path_in(&path, &exe_dir), "{path:?} should contain {exe_dir:?}");

        match previous {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn on_path_in_handles_an_empty_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!on_path_in(&OsString::new(), tmp.path()));
    }
}
