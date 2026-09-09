//! Quoting for command lines handed to `herdr pane run`.
//!
//! `pane run` submits literal text into a pane, which the pane's own shell then
//! parses. That shell is not this process's shell: it is whatever herdr starts
//! panes with, so the quoting rules differ per platform. Getting this wrong is
//! silent - the pane shows a shell error nobody reads.
//!
//! The dialect is a parameter rather than a `cfg`, so both branches are testable
//! on either platform.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneShell {
    /// sh/bash/zsh: single quotes are literal, `'` closes and is re-escaped.
    Posix,
    /// PowerShell: a quoted command needs the `&` call operator, and `'` is
    /// escaped by doubling it.
    PowerShell,
}

impl PaneShell {
    /// The dialect herdr panes use on this platform.
    pub fn host() -> Self {
        if cfg!(windows) {
            PaneShell::PowerShell
        } else {
            PaneShell::Posix
        }
    }

    /// Build a command line that runs `exe` with `args`, safe to pass to
    /// `herdr pane run`.
    pub fn command_line<S: AsRef<str>>(self, exe: &Path, args: &[S]) -> String {
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(self.quote(&exe.to_string_lossy()));
        parts.extend(args.iter().map(|a| self.quote(a.as_ref())));
        let joined = parts.join(" ");
        match self {
            // Without `&`, PowerShell treats a quoted first token as a string
            // expression and just echoes it.
            PaneShell::PowerShell => format!("& {joined}"),
            PaneShell::Posix => joined,
        }
    }

    /// Quote one argument for this dialect.
    pub fn quote(self, arg: &str) -> String {
        match self {
            PaneShell::Posix => format!("'{}'", arg.replace('\'', r"'\''")),
            PaneShell::PowerShell => format!("'{}'", arg.replace('\'', "''")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_command_line_quotes_every_word() {
        let line = PaneShell::Posix.command_line(
            Path::new("/usr/local/bin/horch"),
            &["worker", "sonnet-1"],
        );
        assert_eq!(line, "'/usr/local/bin/horch' 'worker' 'sonnet-1'");
    }

    /// A space in the install path is the common Windows case
    /// (`C:\Users\First Last\...`), and PowerShell needs `&` to run it.
    #[test]
    fn powershell_command_line_uses_the_call_operator() {
        let line = PaneShell::PowerShell.command_line(
            Path::new(r"C:\Users\First Last\horch.exe"),
            &["worker", "sonnet-1"],
        );
        assert_eq!(line, r"& 'C:\Users\First Last\horch.exe' 'worker' 'sonnet-1'");
    }

    #[test]
    fn posix_escapes_embedded_single_quotes() {
        assert_eq!(PaneShell::Posix.quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn powershell_doubles_embedded_single_quotes() {
        assert_eq!(PaneShell::PowerShell.quote("it's"), "'it''s'");
    }

    /// Shell metacharacters in a task string must reach the pane verbatim.
    #[test]
    fn metacharacters_are_neutralised_in_both_dialects() {
        let nasty = "a; rm -rf / && $(echo x) `id` | tee $HOME";
        for shell in [PaneShell::Posix, PaneShell::PowerShell] {
            let quoted = shell.quote(nasty);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
            // No unescaped quote can terminate the literal early.
            assert!(!quoted[1..quoted.len() - 1].contains('\''));
        }
    }

    #[test]
    fn no_args_still_yields_a_runnable_line() {
        assert_eq!(
            PaneShell::Posix.command_line(Path::new("/bin/horch"), &[] as &[&str]),
            "'/bin/horch'"
        );
        assert_eq!(
            PaneShell::PowerShell.command_line(Path::new(r"C:\horch.exe"), &[] as &[&str]),
            r"& 'C:\horch.exe'"
        );
    }
}
