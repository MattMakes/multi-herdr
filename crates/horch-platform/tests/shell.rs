//! The concern shell: the pane shell, the check shell and the case of a path.

use horch_platform::shell::{self, ShellDialect};

#[cfg(unix)]
#[test]
fn check_shell_is_absolute_off_windows() {
    assert_eq!(shell::CHECK_SHELL, "/bin/sh");
}

#[cfg(unix)]
#[test]
fn pane_shell_is_posix_off_windows() {
    assert_eq!(shell::PANE_SHELL, ShellDialect::Posix);
}

#[cfg(windows)]
#[test]
fn pane_shell_is_powershell_on_windows() {
    assert_eq!(shell::PANE_SHELL, ShellDialect::PowerShell);
}

#[cfg(unix)]
#[test]
fn path_case_is_kept_off_windows() {
    assert_eq!(shell::fold_path_case("/Tmp/A"), "/Tmp/A");
}

#[cfg(windows)]
#[test]
fn path_case_is_folded_on_windows() {
    assert_eq!(shell::fold_path_case(r"C:\Tmp\A"), r"c:\tmp\a");
}
