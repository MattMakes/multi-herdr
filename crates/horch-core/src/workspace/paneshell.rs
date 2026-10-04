//! Quoting for command lines handed to `herdr pane run`.
//!
//! `pane run` submits literal text into a pane, which the pane's own shell then
//! parses. That shell is not this process's shell: it is whatever herdr starts
//! panes with, so the quoting rules differ per platform. Getting this wrong is
//! silent - the pane shows a shell error nobody reads.
//!
//! The dialect is a parameter rather than a `cfg`, so both branches are testable
//! on either platform.
//!
//! Every command line first removes [`FORBIDDEN_ENV`]: the herdr server keeps
//! the environment of the shell that started it, so a pane shell can hold
//! `ANTHROPIC_API_KEY`, and no process horch starts in a pane may.

use std::path::Path;

use crate::harness::launch::FORBIDDEN_ENV;

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
    /// `herdr pane run`, without any [`FORBIDDEN_ENV`] variable.
    pub fn command_line<S: AsRef<str>>(self, exe: &Path, args: &[S]) -> String {
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(self.quote(&exe.to_string_lossy()));
        parts.extend(args.iter().map(|a| self.quote(a.as_ref())));
        let joined = parts.join(" ");
        match self {
            // `Env:` is the PowerShell process's own environment, so the pane
            // shell loses the variable too. Without `&`, PowerShell treats a
            // quoted first token as a string expression and just echoes it.
            PaneShell::PowerShell => {
                let removes: String = FORBIDDEN_ENV
                    .iter()
                    .map(|name| format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue; "))
                    .collect();
                format!("{removes}& {joined}")
            }
            // An absolute `env`: a pane's PATH is not this process's PATH.
            // BSD, GNU and busybox `env` all take `-u`.
            PaneShell::Posix => {
                let removes: String = FORBIDDEN_ENV
                    .iter()
                    .map(|name| format!(" -u {name}"))
                    .collect();
                format!("/usr/bin/env{removes} {joined}")
            }
        }
    }

    /// Quote one argument for this dialect.
    pub(crate) fn quote(self, arg: &str) -> String {
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
        let line = PaneShell::Posix
            .command_line(Path::new("/usr/local/bin/horch"), &["worker", "sonnet-1"]);
        assert_eq!(
            line,
            "/usr/bin/env -u ANTHROPIC_API_KEY '/usr/local/bin/horch' 'worker' 'sonnet-1'"
        );
    }

    /// A space in the install path is the common Windows case
    /// (`C:\Users\First Last\...`), and PowerShell needs `&` to run it.
    #[test]
    fn powershell_command_line_uses_the_call_operator() {
        let line = PaneShell::PowerShell.command_line(
            Path::new(r"C:\Users\First Last\horch.exe"),
            &["worker", "sonnet-1"],
        );
        assert_eq!(
            line,
            r"Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; & 'C:\Users\First Last\horch.exe' 'worker' 'sonnet-1'"
        );
    }

    /// The herdr server carries the operator's environment into every pane
    /// shell, so each line removes every forbidden name before it runs
    /// anything.
    #[test]
    fn every_command_line_first_removes_every_forbidden_name() {
        for name in FORBIDDEN_ENV {
            let posix = PaneShell::Posix.command_line(Path::new("/bin/horch"), &["worker"]);
            assert!(posix.starts_with("/usr/bin/env -u "), "{posix}");
            assert!(posix.contains(&format!(" -u {name} ")), "{posix}");
            let ps = PaneShell::PowerShell.command_line(Path::new(r"C:\horch.exe"), &["worker"]);
            let remove = format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue; ");
            assert!(ps.contains(&remove), "{ps}");
            assert!(ps.find(&remove) < ps.find("& "), "{ps}");
        }
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
            "/usr/bin/env -u ANTHROPIC_API_KEY '/bin/horch'"
        );
        assert_eq!(
            PaneShell::PowerShell.command_line(Path::new(r"C:\horch.exe"), &[] as &[&str]),
            r"Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; & 'C:\horch.exe'"
        );
    }
}
