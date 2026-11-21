//! The low-level git runner. Every git process this crate starts goes
//! through `GitRunner::run` or `GitRunner::output`, in an explicit directory,
//! with prompts, system config and hooks off. The core crate's VCS layer
//! wraps this module later, so it stays general: any argv, any directory.
//! [`GitRunner::with_sandbox`] runs each child in the platform sandbox.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use horch_platform::group::{self, Contained};
use horch_platform::sandbox::{Denials, Network, Reach};
use horch_platform::start::{self, Command};
use horch_platform::NotSupported;

/// Removed from every git child, so the `-C` directory alone selects the
/// repository and its config. A caller inside a git hook, or under
/// `git rebase --exec`, has `GIT_DIR` set: on 2026-10-03 a gate run under
/// `git rebase -x` let test fixtures `git init` and commit into the real
/// repository. Every git child that horch or its tests start removes these
/// with [`scrub_repo_env`].
///
/// The list holds every name of `git rev-parse --local-env-vars` (a test
/// checks the host git), plus `GIT_NAMESPACE` and `GIT_CEILING_DIRECTORIES`.
/// [`scrub_repo_env`] also removes the numbered `GIT_CONFIG_KEY_<n>` and
/// `GIT_CONFIG_VALUE_<n>`.
pub const REPO_ENV: [&str; 18] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CEILING_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NAMESPACE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// The prefixes of the numbered config variables that `GIT_CONFIG_COUNT`
/// counts.
pub const REPO_ENV_NUMBERED: [&str; 2] = ["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"];

/// A git child that [`scrub_repo_env`] cleans: a [`start::Command`], or
/// the `std` command of a caller that has not moved to it yet (SR-7).
pub trait GitChild {
    /// The names that the command sets or removes.
    fn env_names(&self) -> Vec<OsString>;
    fn remove_env(&mut self, key: &OsStr);
}

impl GitChild for Command {
    fn env_names(&self) -> Vec<OsString> {
        self.get_envs().map(|(k, _)| k.to_os_string()).collect()
    }
    fn remove_env(&mut self, key: &OsStr) {
        self.env_remove(key);
    }
}

impl GitChild for std::process::Command {
    fn env_names(&self) -> Vec<OsString> {
        self.get_envs().map(|(k, _)| k.to_os_string()).collect()
    }
    fn remove_env(&mut self, key: &OsStr) {
        self.env_remove(key);
    }
}

/// Remove every [`REPO_ENV`] variable and every `GIT_CONFIG_KEY_<n>` and
/// `GIT_CONFIG_VALUE_<n>` from `cmd`, a git child: those this process has
/// and those set on `cmd`. Call it after any `env` call.
pub fn scrub_repo_env(cmd: &mut impl GitChild) {
    for k in REPO_ENV {
        cmd.remove_env(OsStr::new(k));
    }
    let numbered: Vec<OsString> = std::env::vars_os()
        .map(|(k, _)| k)
        .chain(cmd.env_names())
        .filter(|k| {
            k.to_str()
                .is_some_and(|k| REPO_ENV_NUMBERED.iter().any(|p| k.starts_with(p)))
        })
        .collect();
    for k in numbered {
        cmd.remove_env(&k);
    }
}

/// Set on every git child.
const FIXED_ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

/// Passed as `-c` before every subcommand.
const FIXED_CONFIG: [&str; 1] = ["core.hooksPath=/dev/null"];

/// The files that show which repository `git_dir` is: its config, `HEAD`,
/// `packed-refs` and every loose ref, as one text. The [`REPO_ENV`]
/// regression tests compare it before and after git runs with `GIT_DIR`
/// aimed at `git_dir`.
pub fn repo_state(git_dir: &Path) -> String {
    let mut out = String::new();
    for name in ["config", "HEAD", "packed-refs"] {
        out += &format!(
            "{name}: {:?}\n",
            std::fs::read_to_string(git_dir.join(name)).ok()
        );
    }
    let mut stack = vec![git_dir.join("refs")];
    let mut refs = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                refs.push(format!(
                    "{}: {:?}",
                    e.path().display(),
                    std::fs::read_to_string(e.path()).ok()
                ));
            }
        }
    }
    refs.sort();
    out + &refs.join("\n")
}

/// The sandbox host: it wraps a command so that it reaches only what a
/// [`Reach`] allows: it writes below the writable paths, reads the whole
/// disk but the hidden paths, can read the readable paths in them again,
/// and cannot read, write or connect to a Unix socket below a hidden path.
/// [`horch_platform::sandbox::sandboxed_hiding`] is the real one; a test
/// gives a fake answer.
pub type SandboxHost = fn(Command, &Reach<'_>) -> Result<Command, NotSupported>;

/// The variables of this process that a sandboxed git gets. It gets no
/// other variable of this process, so a program that git starts cannot
/// read a secret of fleetd from its environment.
pub const SANDBOX_KEPT_ENV: [&str; 9] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TZ",
];

/// The whole environment of the sandboxed `cmd`: the variables of
/// [`SANDBOX_KEPT_ENV`] of this process, then the changes of `cmd`.
fn sandboxed_env(cmd: &Command) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = SANDBOX_KEPT_ENV
        .iter()
        .filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v)))
        .collect();
    for (key, value) in cmd.get_envs() {
        env.retain(|(k, _)| k != key);
        if let Some(value) = value {
            env.push((key.to_owned(), value.to_owned()));
        }
    }
    env
}

/// The sandbox of every child: what it can reach (see [`SandboxHost`]),
/// its network rule, and the host that wraps it. The child gets only the
/// environment of [`SANDBOX_KEPT_ENV`] and of the runner.
#[derive(Debug, Clone)]
pub struct GitSandbox {
    pub writable: Vec<PathBuf>,
    pub readable: Vec<PathBuf>,
    pub hidden: Vec<PathBuf>,
    /// The acts that the child cannot do, whatever the paths allow.
    pub denied: Denials,
    pub network: Network,
    pub host: SandboxHost,
}

#[derive(Debug, Clone)]
pub struct GitRunner {
    bin: PathBuf,
    env: Vec<(OsString, OsString)>,
    /// The `GIT_INDEX_FILE` of every child, set after the scrub.
    index: Option<PathBuf>,
    /// The sandbox of every child, when it has one.
    sandbox: Option<GitSandbox>,
    limit: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    /// The exit code; -1 when a signal ended git.
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl GitOutput {
    /// Stdout as UTF-8 (lossy), with the trailing newline trimmed.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim_end().to_owned()
    }
}

#[derive(Debug)]
pub enum GitError {
    /// `run` was called with an empty directory path.
    NoDir,
    /// `with_index_file` was given a path that is not absolute.
    RelativeIndex(PathBuf),
    /// The runner has a sandbox and this host has none, so git did not run.
    NoSandbox(NotSupported),
    /// Git ran past the time limit of the runner and was stopped: `SIGTERM`
    /// to its group, then `SIGKILL` to the group after [`TERM_GRACE`].
    /// `by_term`: the child ended in the grace, so a git that got the
    /// signal could remove its `.lock` files. Else the `SIGKILL` ended it,
    /// and a `.lock` file can stay. In a sandbox the child can be the
    /// sandbox and not git: then only the `SIGKILL` of the group ends git.
    TimedOut {
        args: Vec<String>,
        limit: Duration,
        by_term: bool,
    },
    Spawn {
        bin: PathBuf,
        source: io::Error,
    },
    Failed {
        args: Vec<String>,
        status: i32,
        stderr: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDir => write!(f, "git needs an explicit working directory"),
            Self::RelativeIndex(path) => {
                write!(
                    f,
                    "a private index file needs an absolute path, not {}",
                    path.display()
                )
            }
            Self::NoSandbox(gap) => write!(f, "no sandbox: {gap}"),
            Self::TimedOut {
                args,
                limit,
                by_term,
            } => {
                write!(
                    f,
                    "git {} ran past its time limit of {} s and was stopped: ",
                    args.join(" "),
                    limit.as_secs()
                )?;
                if *by_term {
                    write!(
                        f,
                        "it ended in {} s after SIGTERM to its group, then SIGKILL ended the rest of its group",
                        TERM_GRACE.as_secs()
                    )
                } else {
                    write!(
                        f,
                        "it did not end in {} s after SIGTERM, so SIGKILL ended its group, \
                         and a .lock file of git can stay",
                        TERM_GRACE.as_secs()
                    )
                }
            }
            Self::Spawn { bin, source } => write!(f, "cannot run {}: {source}", bin.display()),
            Self::Failed {
                args,
                status,
                stderr,
            } => write!(f, "git {} failed ({status}): {stderr}", args.join(" ")),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl GitRunner {
    /// `bin` is the git executable. The caller resolves it (for example from
    /// `HORCH_GIT_BIN`); this crate never reads the process environment.
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            env: Vec::new(),
            index: None,
            sandbox: None,
            limit: None,
        }
    }

    /// Stop every child at `limit`: `SIGTERM` to its own group first, so
    /// that git removes its `.lock` files, then `SIGKILL` to the group after
    /// [`TERM_GRACE`]. The result is [`GitError::TimedOut`]. Each child then
    /// leads a group that this runner made for it ([`Contained`]), so the
    /// stop reaches no other process. With no limit a child runs until it
    /// ends.
    pub fn with_time_limit(mut self, limit: Duration) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Run every child in `sandbox`. A host with no sandbox gives
    /// [`GitError::NoSandbox`], and git does not run.
    pub fn with_sandbox(mut self, sandbox: GitSandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Add one variable to every child, for example `GIT_CONFIG_GLOBAL` in
    /// a hermetic test. The fixed variables still win.
    pub fn with_env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// Give every child the private index file `path` as `GIT_INDEX_FILE`.
    /// The scrub removes a `GIT_INDEX_FILE` of the environment or of
    /// [`GitRunner::with_env`]; this one is set after the scrub. The caller
    /// owns `path`, keeps it outside every project, and deletes the file
    /// when it is done. A path that is not absolute is an error.
    pub fn with_index_file(mut self, path: impl Into<PathBuf>) -> Result<Self, GitError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(GitError::RelativeIndex(path));
        }
        self.index = Some(path);
        Ok(self)
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// Run `git <args>` in `dir` and return its output whatever the exit code.
    pub fn output(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        self.output_with(dir, args, &mut group::exited_without_reap)
    }

    /// [`Self::output`] with `exited`, the look at a limited child that
    /// does not reap it: the seam of the tests of D254-F2.
    fn output_with(
        &self,
        dir: &Path,
        args: &[&str],
        exited: &mut dyn FnMut(&mut Child) -> io::Result<bool>,
    ) -> Result<GitOutput, GitError> {
        if dir.as_os_str().is_empty() {
            return Err(GitError::NoDir);
        }
        let mut cmd = start::command(&self.bin);
        cmd.current_dir(dir);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        for (k, v) in FIXED_ENV {
            cmd.env(k, v);
        }
        scrub_repo_env(&mut cmd);
        if let Some(index) = &self.index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        // The directory again as `-C`, so the repository never depends on
        // how git searches from its working directory.
        let at = std::path::absolute(dir).map_err(|source| GitError::Spawn {
            bin: self.bin.clone(),
            source,
        })?;
        cmd.arg("-C").arg(at);
        for c in FIXED_CONFIG {
            cmd.arg("-c").arg(c);
        }
        cmd.args(args);
        // The sandbox keeps the program, arguments, environment and folder
        // of `cmd`, so stdio is set on the command that runs.
        if let Some(sandbox) = &self.sandbox {
            let env = sandboxed_env(&cmd);
            let reach = Reach {
                writable: &sandbox.writable,
                readable: &sandbox.readable,
                hidden: &sandbox.hidden,
                denied: &sandbox.denied,
                network: sandbox.network,
            };
            cmd = (sandbox.host)(cmd, &reach).map_err(GitError::NoSandbox)?;
            cmd.env_clear().envs(env);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawn = |source| GitError::Spawn {
            bin: self.bin.clone(),
            source,
        };
        let Some(limit) = self.limit else {
            let out = cmd.output().map_err(spawn)?;
            return Ok(GitOutput {
                status: out.status.code().unwrap_or(-1),
                stdout: out.stdout,
                stderr: out.stderr,
            });
        };
        match output_within(&mut cmd, limit, exited).map_err(spawn)? {
            Limited::Ended(out) => Ok(out),
            Limited::Stopped { by_term } => Err(GitError::TimedOut {
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                limit,
                by_term,
            }),
        }
    }

    /// Run `git <args>` in `dir`; a non-zero exit is an error.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        let out = self.output(dir, args)?;
        if out.status != 0 {
            return Err(GitError::Failed {
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        Ok(out)
    }
}

/// The wait after the `SIGTERM` of a child at its time limit, before the
/// `SIGKILL` of its group. Git removes its `.lock` files at once on
/// `SIGTERM`.
pub const TERM_GRACE: Duration = Duration::from_secs(1);

/// The longest sleep between 2 looks at a child under a time limit. The
/// first look comes after 1 ms, so a short git call waits little.
const LOOK_MAX: Duration = Duration::from_millis(20);

enum Limited {
    Ended(GitOutput),
    Stopped { by_term: bool },
}

/// The child of a limited git and its group, held from the spawn to the
/// reap. A drop while armed kills the group and reaps the child, so an
/// early return leaves no git that keeps a `.lock` (D254-F2).
struct Held {
    child: Child,
    contained: Contained,
    armed: bool,
}

impl Held {
    /// The reap of the child, then no kill at the drop: with the child
    /// reaped, its pid can go to another process.
    fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        let status = self.child.wait()?;
        self.armed = false;
        Ok(status)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.armed {
            self.contained.terminate();
            group::kill_child_group(&mut self.child);
            let _ = self.child.wait();
        }
    }
}

/// Run `cmd` with its stdout and stderr read, and stop its group at
/// `limit`. The child stays unreaped until its group is stopped, so its pid
/// names its group and no other process. `exited` looks at the child
/// without a reap; an `EINTR` of it is retried (D254-F2).
fn output_within(
    cmd: &mut Command,
    limit: Duration,
    exited: &mut dyn FnMut(&mut Child) -> io::Result<bool>,
) -> io::Result<Limited> {
    let deadline = Instant::now() + limit;
    Contained::prepare(cmd);
    let mut child = cmd.spawn()?;
    let contained = match Contained::start(&child) {
        Ok(contained) => contained,
        Err(e) => {
            group::kill_child_group(&mut child);
            let _ = child.wait();
            return Err(e);
        }
    };
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let mut held = Held {
        child,
        contained,
        armed: true,
    };
    let mut exited = |child: &mut Child| loop {
        match exited(child) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    };
    let mut look = Duration::from_millis(1);
    let ended = loop {
        // The readers end when each process with the pipes ends, also a
        // process that git leaves in its group.
        if exited(&mut held.child)? && stdout.is_finished() && stderr.is_finished() {
            break true;
        }
        let now = Instant::now();
        if now >= deadline {
            break false;
        }
        std::thread::sleep(look.min(deadline - now));
        look = (look * 2).min(LOOK_MAX);
    };
    if ended {
        let status = held.wait()?;
        return Ok(Limited::Ended(GitOutput {
            status: status.code().unwrap_or(-1),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        }));
    }
    let by_term = stop(&mut held, &mut exited)?;
    let _ = held.wait();
    let _ = stdout.join();
    let _ = stderr.join();
    Ok(Limited::Stopped { by_term })
}

/// `SIGTERM` to the group of the child of `held`, a wait of [`TERM_GRACE`]
/// for the child to end, then `SIGKILL` to the group. The kill also comes
/// when the child ended in the grace, for a process that it left in its
/// group. Whether the child ended in the grace.
fn stop(
    held: &mut Held,
    exited: &mut dyn FnMut(&mut Child) -> io::Result<bool>,
) -> io::Result<bool> {
    let grace_end = Instant::now() + TERM_GRACE;
    let mut by_term = false;
    if group::term_child_group(&mut held.child) {
        while Instant::now() < grace_end {
            if exited(&mut held.child)? {
                by_term = true;
                break;
            }
            std::thread::sleep(LOOK_MAX);
        }
    }
    held.contained.terminate();
    group::kill_child_group(&mut held.child);
    Ok(by_term)
}

fn read_all(pipe: Option<impl io::Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: Command, _: &Reach<'_>) -> Result<Command, NotSupported> {
        Err(NotSupported::new("sandbox", "test").because("no host"))
    }

    fn same(cmd: Command, _: &Reach<'_>) -> Result<Command, NotSupported> {
        Ok(cmd)
    }

    #[test]
    fn a_sandboxed_git_gets_only_the_kept_variables_of_this_process() {
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "cargo sets it"
        );
        let dir = tempfile::tempdir().unwrap();
        let git = GitRunner::new("git")
            .with_env("HORCH_TEST_SET", "kept")
            .with_sandbox(sandbox(same));
        git.run(dir.path(), &["init", "-q"]).unwrap();
        let out = git
            .run(dir.path(), &["-c", "alias.vars=!env", "vars"])
            .unwrap();
        let vars = String::from_utf8_lossy(&out.stdout);
        assert!(!vars.contains("CARGO_MANIFEST_DIR="), "{vars}");
        assert!(vars.contains("HORCH_TEST_SET=kept"), "{vars}");
        assert!(vars.lines().any(|l| l.starts_with("PATH=")), "{vars}");
    }

    fn sandbox(host: SandboxHost) -> GitSandbox {
        GitSandbox {
            writable: Vec::new(),
            readable: Vec::new(),
            hidden: Vec::new(),
            denied: Denials::default(),
            network: Network::Denied,
            host,
        }
    }

    #[test]
    fn a_host_with_no_sandbox_runs_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let runner = GitRunner::new("git").with_sandbox(sandbox(none));
        let e = runner.output(dir.path(), &["init", "-q"]).unwrap_err();
        assert!(matches!(e, GitError::NoSandbox(_)), "{e:?}");
        assert!(e.to_string().starts_with("no sandbox: "), "{e}");
        assert!(!dir.path().join(".git").exists(), "git did not run");
    }

    /// A fake git in `dir`: the shell `body`, after it records its pid and
    /// the pid of its child `sleep 600` in `pids`.
    fn fake_git(dir: &Path, body: &str) -> (PathBuf, PathBuf) {
        let pids = dir.join("pids");
        let bin = dir.join("fake-git");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n{body}\nsleep 600 &\necho \"$$ $!\" > '{pids}.new'\nmv '{pids}.new' '{pids}'\nwait\n",
                pids = pids.display()
            ),
        )
        .unwrap();
        horch_platform::fs::make_executable(&bin).unwrap();
        (bin, pids)
    }

    fn all_gone(pids: &Path) {
        let text = std::fs::read_to_string(pids).expect("the fake git started");
        let until = Instant::now() + Duration::from_secs(5);
        for pid in text.split_whitespace() {
            let pid: u32 = pid.parse().unwrap();
            while horch_platform::process::start_time(pid).is_some() {
                assert!(Instant::now() < until, "the process {pid} of git lives on");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// No process group on Windows, and the fake git is a shell script.
    fn no_groups() -> bool {
        group::kill_group_id(0).is_err()
    }

    /// A run of the fake git of `dir` with `body`, with a time limit of
    /// 1 s. The first look at the child waits for the pids file, the ready
    /// mark that the fake writes after `body`: on a loaded host the shell
    /// can start later than the limit, and a TERM before its `trap` ends it.
    /// That first look then gives `error`, or looks at the child. The
    /// result, the pids file, and the time of the ready mark.
    fn run_when_ready(
        dir: &Path,
        body: &str,
        error: Option<io::ErrorKind>,
    ) -> (Result<GitOutput, GitError>, PathBuf, Instant) {
        let (bin, pids) = fake_git(dir, body);
        let mut ready = None;
        let mut exited = |child: &mut Child| {
            if ready.is_none() {
                let until = Instant::now() + Duration::from_secs(30);
                while !pids.is_file() {
                    assert!(Instant::now() < until, "the fake git records its pids");
                    std::thread::sleep(Duration::from_millis(10));
                }
                ready = Some(Instant::now());
                if let Some(error) = error {
                    return Err(io::Error::from(error));
                }
            }
            group::exited_without_reap(child)
        };
        let got = GitRunner::new(&bin)
            .with_time_limit(Duration::from_secs(1))
            .output_with(dir, &["update-ref", "x"], &mut exited);
        (got, pids, ready.expect("the fake git is ready"))
    }

    #[test]
    fn a_git_past_its_time_limit_ends_by_term_with_its_group() {
        if no_groups() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (got, pids, ready) = run_when_ready(dir.path(), "trap 'exit 143' TERM", None);
        let took = ready.elapsed();
        let e = got.unwrap_err();
        assert!(
            matches!(e, GitError::TimedOut { by_term: true, .. }),
            "{e:?}"
        );
        assert!(
            took < Duration::from_secs(2),
            "the TERM ends it at once: {took:?}"
        );
        assert!(
            e.to_string()
                .contains("git update-ref x ran past its time limit of 1 s"),
            "{e}"
        );
        all_gone(&pids);
    }

    #[test]
    fn a_git_that_ignores_term_is_killed_after_the_grace() {
        if no_groups() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        // The child `sleep` gets the ignored TERM too.
        let (got, pids, _) = run_when_ready(dir.path(), "trap '' TERM", None);
        let took = started.elapsed();
        let e = got.unwrap_err();
        assert!(
            matches!(e, GitError::TimedOut { by_term: false, .. }),
            "{e:?}"
        );
        assert!(
            took >= Duration::from_secs(1) + TERM_GRACE,
            "the KILL waits for the grace: {took:?}"
        );
        assert!(
            e.to_string().contains("a .lock file of git can stay"),
            "{e}"
        );
        all_gone(&pids);
    }

    #[test]
    fn an_interrupted_wait_still_stops_and_reaps_the_limited_git() {
        if no_groups() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let interrupted = Some(io::ErrorKind::Interrupted);
        let (got, pids, _) = run_when_ready(dir.path(), "trap 'exit 143' TERM", interrupted);
        assert!(
            matches!(got, Err(GitError::TimedOut { by_term: true, .. })),
            "the wait goes on after EINTR: {got:?}"
        );
        all_gone(&pids);
    }

    #[test]
    fn an_error_of_the_wait_kills_and_reaps_the_limited_git() {
        if no_groups() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let other = Some(io::ErrorKind::Other);
        let (got, pids, _) = run_when_ready(dir.path(), "trap 'exit 143' TERM", other);
        assert!(matches!(got, Err(GitError::Spawn { .. })), "{got:?}");
        all_gone(&pids);
    }

    #[test]
    fn a_git_within_its_time_limit_gives_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let out = GitRunner::new("git")
            .with_time_limit(Duration::from_secs(10))
            .run(dir.path(), &["--version"])
            .unwrap();
        assert!(out.stdout_text().starts_with("git version"), "{out:?}");
    }
}
