use crate::os;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    Posix,
    PowerShell,
}

pub const PANE_SHELL: ShellDialect = os::shell::PANE_SHELL;

pub const CHECK_SHELL: &str = os::shell::CHECK_SHELL;

pub fn fold_path_case(path: &str) -> String {
    os::shell::fold_path_case(path)
}
