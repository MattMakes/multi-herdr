//! The concern shell for Windows.

use crate::shell::ShellDialect;

pub(crate) const PANE_SHELL: ShellDialect = ShellDialect::PowerShell;

pub(crate) const CHECK_SHELL: &str = "sh";

/// The Windows file system is case-blind: compare paths in lowercase.
pub(crate) fn fold_path_case(path: &str) -> String {
    path.to_ascii_lowercase()
}
