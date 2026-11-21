//! The concern start: the only way, outside the launcher of fleetd, to start
//! a process (SR-7). A process that [`Command`] starts never has a name of
//! [`FORBIDDEN_ENV`]: each start removes each of them just before the OS
//! starts the child, so no earlier `env` call can put one back.
//!
//! Build a command with [`command`]. The concerns `group`, `sandbox` and
//! `rest` take a command of the trait [`Start`]: a [`Command`], or a
//! `std::process::Command` of a caller that has not moved to this file yet.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Child, ExitStatus, Output, Stdio};

/// Environment variables that no child process ever receives. The rule of
/// the operator (CLAUDE.md): nothing in a fleet uses `ANTHROPIC_API_KEY`.
/// Claude panes sign in with the claude.ai login of the operator. The shell
/// that starts a fleet can export the key, so each start removes it.
pub const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// A command that runs `program`. The same as `std::process::Command::new`.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command(std::process::Command::new(program))
}

/// A `std::process::Command` that removes each name of [`FORBIDDEN_ENV`]
/// just before [`spawn`](Self::spawn), [`output`](Self::output) and
/// [`status`](Self::status). Build it with [`command`]. Each builder method
/// is the method of `std::process::Command` with the same name.
#[derive(Debug)]
pub struct Command(std::process::Command);

impl Command {
    /// A verbatim path (`\\?\C:\dir` on Windows) goes to the child in its
    /// plain form, as [`crate::path::for_child`] gives it: git and other
    /// programs refuse the verbatim form.
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(crate::os::path::for_child(arg.as_ref()));
        self
    }

    /// Each argument as [`arg`](Self::arg) gives it.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    /// A name of [`FORBIDDEN_ENV`] has no effect: each start removes it.
    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.0.env(key, val);
        self
    }

    /// A name of [`FORBIDDEN_ENV`] has no effect: each start removes it.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.envs(vars);
        self
    }

    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.0.env_remove(key);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.0.env_clear();
        self
    }

    /// The variables that the command sets (`Some`) or removes (`None`).
    pub fn get_envs(&self) -> std::process::CommandEnvs<'_> {
        self.0.get_envs()
    }

    pub fn get_program(&self) -> &OsStr {
        self.0.get_program()
    }

    pub fn get_args(&self) -> std::process::CommandArgs<'_> {
        self.0.get_args()
    }

    pub fn get_current_dir(&self) -> Option<&Path> {
        self.0.get_current_dir()
    }

    /// A verbatim folder goes to the child in its plain form, as
    /// [`arg`](Self::arg).
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.0.current_dir(crate::path::for_child(dir.as_ref()));
        self
    }

    pub fn stdin(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdin(cfg);
        self
    }

    pub fn stdout(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stdout(cfg);
        self
    }

    pub fn stderr(&mut self, cfg: impl Into<Stdio>) -> &mut Self {
        self.0.stderr(cfg);
        self
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        self.scrubbed().spawn()
    }

    pub fn output(&mut self) -> io::Result<Output> {
        self.scrubbed().output()
    }

    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.scrubbed().status()
    }

    /// The inner command, with each name of [`FORBIDDEN_ENV`] removed. Call
    /// it just before the start, after the last `env` call.
    fn scrubbed(&mut self) -> &mut std::process::Command {
        for key in FORBIDDEN_ENV {
            self.0.env_remove(key);
        }
        &mut self.0
    }
}

/// A command that a concern of this crate can prepare or start: a
/// [`Command`], or a `std::process::Command`. The trait is sealed: only
/// this crate reaches the inner `std` command.
pub trait Start: sealed::Sealed {}

impl Start for Command {}
impl Start for std::process::Command {}

pub(crate) use sealed::{Sealed, TOKEN};

/// The `std` command inside a [`Command`], for the concerns of this crate.
pub(crate) type StdCommand = std::process::Command;

mod sealed {
    /// The proof that the caller is this crate. Code outside the crate
    /// cannot build it, so it cannot call a method of [`Sealed`].
    pub struct Token(());

    pub(crate) const TOKEN: Token = Token(());

    /// The access of this crate to the inner `std` command.
    pub trait Sealed: Sized {
        fn std(&self, token: Token) -> &std::process::Command;
        fn std_mut(&mut self, token: Token) -> &mut std::process::Command;
        /// The inner command. A [`super::Command`] gives it with each
        /// forbidden name removed, ready to start.
        fn into_std(self, token: Token) -> std::process::Command;
        /// A command of this kind that runs `cmd`.
        fn from_std(cmd: std::process::Command, token: Token) -> Self;
    }

    impl Sealed for super::Command {
        fn std(&self, _: Token) -> &std::process::Command {
            &self.0
        }
        fn std_mut(&mut self, _: Token) -> &mut std::process::Command {
            &mut self.0
        }
        fn into_std(mut self, _: Token) -> std::process::Command {
            self.scrubbed();
            self.0
        }
        fn from_std(cmd: std::process::Command, _: Token) -> Self {
            super::Command(cmd)
        }
    }

    impl Sealed for std::process::Command {
        fn std(&self, _: Token) -> &std::process::Command {
            self
        }
        fn std_mut(&mut self, _: Token) -> &mut std::process::Command {
            self
        }
        fn into_std(self, _: Token) -> std::process::Command {
            self
        }
        fn from_std(cmd: std::process::Command, _: Token) -> Self {
            cmd
        }
    }
}
