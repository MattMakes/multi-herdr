//! The concern shell: the shell and quoting.
//!
//! The facts of the shell that core code needs: the dialect of a pane
//! shell, the shell that runs a check, and whether the file system folds
//! the case of a path.

use crate::os;

/// The quoting rules of a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    /// sh, bash or zsh: single quotes are literal.
    Posix,
    /// PowerShell: a quoted command needs the `&` call operator, and `'` is
    /// escaped by doubling it.
    PowerShell,
}

/// The dialect of the shell of a herdr pane: PowerShell on Windows, Posix
/// elsewhere.
pub const PANE_SHELL: ShellDialect = os::shell::PANE_SHELL;

/// The shell that runs a check: `/bin/sh` off Windows, an absolute path
/// because a pane's PATH may not reach a shell. `sh` on Windows.
pub const CHECK_SHELL: &str = os::shell::CHECK_SHELL;

/// `path` in the form to compare it in: lowercase (ASCII) on Windows,
/// whose file system is case-blind, and unchanged elsewhere.
pub fn fold_path_case(path: &str) -> String {
    os::shell::fold_path_case(path)
}
