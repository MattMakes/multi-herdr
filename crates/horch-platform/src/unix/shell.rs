use crate::shell::ShellDialect;

pub(crate) const PANE_SHELL: ShellDialect = ShellDialect::Posix;

pub(crate) const CHECK_SHELL: &str = "/bin/sh";

pub(crate) fn fold_path_case(path: &str) -> String {
    path.to_owned()
}
