//! The concern shell for the code of macOS and Linux.

use crate::shell::ShellDialect;

pub(crate) const PANE_SHELL: ShellDialect = ShellDialect::Posix;

pub(crate) const CHECK_SHELL: &str = "/bin/sh";

/// A unix file system keeps the case of a path.
pub(crate) fn fold_path_case(path: &str) -> String {
    path.to_owned()
}
