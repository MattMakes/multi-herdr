use crate::shell::ShellDialect;

pub(crate) const PANE_SHELL: ShellDialect = ShellDialect::PowerShell;

pub(crate) const CHECK_SHELL: &str = "sh";

pub(crate) fn fold_path_case(path: &str) -> String {
    path.to_ascii_lowercase()
}
