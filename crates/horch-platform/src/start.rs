use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Child, ExitStatus, Output, Stdio};

pub const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];

pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command(std::process::Command::new(program))
}

#[derive(Debug)]
pub struct Command(std::process::Command);

impl Command {
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(crate::os::path::for_child(arg.as_ref()));
        self
    }

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

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.0.env(key, crate::os::path::for_child(val.as_ref()));
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (key, val) in vars {
            self.env(key, val);
        }
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

    fn scrubbed(&mut self) -> &mut std::process::Command {
        for key in FORBIDDEN_ENV {
            self.0.env_remove(key);
        }
        &mut self.0
    }
}

pub trait Start: sealed::Sealed {}

impl Start for Command {}
impl Start for std::process::Command {}

pub(crate) use sealed::{Sealed, TOKEN};

pub(crate) type StdCommand = std::process::Command;

mod sealed {
    pub struct Token(());

    pub(crate) const TOKEN: Token = Token(());

    pub trait Sealed: Sized {
        fn std(&self, token: Token) -> &std::process::Command;
        fn std_mut(&mut self, token: Token) -> &mut std::process::Command;
        fn into_std(self, token: Token) -> std::process::Command;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_env_value_reaches_the_child_in_its_plain_form() {
        let mut cmd = command("git");
        cmd.env("GIT_INDEX_FILE", r"\\?\C:\t\scratch-index")
            .envs([("GIT_DIR", r"\\?\UNC\s\h\.git"), ("GIT_EDITOR", "true")]);
        let mut envs: Vec<(String, String)> = cmd
            .get_envs()
            .map(|(k, v)| {
                let text = |s: &OsStr| s.to_string_lossy().into_owned();
                (text(k), v.map(text).unwrap_or_default())
            })
            .collect();
        envs.sort();
        let (index, dir) = if cfg!(windows) {
            (r"C:\t\scratch-index", r"\\s\h\.git")
        } else {
            (r"\\?\C:\t\scratch-index", r"\\?\UNC\s\h\.git")
        };
        let want = [
            ("GIT_DIR", dir),
            ("GIT_EDITOR", "true"),
            ("GIT_INDEX_FILE", index),
        ];
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(envs, want);
    }
}
