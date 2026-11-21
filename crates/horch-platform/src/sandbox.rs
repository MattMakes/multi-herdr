use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::os::sandbox as os;
use crate::start::{self, Sealed, Start, StdCommand, TOKEN};
use crate::NotSupported;

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub fn sandboxed<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
) -> Result<C, NotSupported> {
    sandboxed_on(cmd, writable, denied, Network::Open, false)
}

pub fn sandboxed_job<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
) -> Result<C, NotSupported> {
    sandboxed_on(cmd, writable, denied, Network::Open, true)
}

pub fn sandboxed_local<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
) -> Result<C, NotSupported> {
    sandboxed_on(cmd, writable, denied, Network::Local, true)
}

fn sandboxed_on<C: Start>(
    cmd: C,
    writable: &[PathBuf],
    denied: &Denials,
    network: Network,
    own_pids: bool,
) -> Result<C, NotSupported> {
    let temp = resolved(&std::env::temp_dir());
    let reach = Reach {
        writable,
        readable: &[],
        hidden: &[],
        denied,
        network,
    };
    wrapped(cmd, &reach, Some(&temp), own_pids)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Open,
    Denied,
    Local,
}

#[derive(Debug, Clone, Copy)]
pub struct Reach<'a> {
    pub writable: &'a [PathBuf],
    pub readable: &'a [PathBuf],
    pub hidden: &'a [PathBuf],
    pub denied: &'a Denials,
    pub network: Network,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Denials {
    pub read: Vec<PathBuf>,
    pub connect: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
}

pub fn sandboxed_hiding<C: Start>(cmd: C, reach: &Reach<'_>) -> Result<C, NotSupported> {
    wrapped(cmd, reach, None, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rule<'a> {
    Read(&'a Path),
    Write(&'a Path),
    Hide(&'a Path),
}

impl<'a> Rule<'a> {
    pub(crate) fn path(&self) -> &'a Path {
        match *self {
            Rule::Read(path) | Rule::Write(path) | Rule::Hide(path) => path,
        }
    }

    fn key(&self) -> (usize, u8) {
        let (path, rank) = match self {
            Rule::Read(path) => (path, 0),
            Rule::Write(path) => (path, 1),
            Rule::Hide(path) => (path, 2),
        };
        (path.components().count(), rank)
    }
}

fn wrapped<C: Start>(
    cmd: C,
    reach: &Reach<'_>,
    temp: Option<&Path>,
    own_pids: bool,
) -> Result<C, NotSupported> {
    let cmd = cmd.std(TOKEN);
    let program = os::program(path_of(cmd).as_deref())?;
    let all = |paths: &[PathBuf]| paths.iter().map(|p| resolved(p)).collect::<Vec<_>>();
    let (writable, readable, hidden) =
        (all(reach.writable), all(reach.readable), all(reach.hidden));
    let mut rules: Vec<Rule<'_>> = readable.iter().map(|p| Rule::Read(p)).collect();
    rules.extend(writable.iter().map(|p| Rule::Write(p)));
    rules.extend(temp.map(Rule::Write));
    rules.extend(hidden.iter().map(|p| Rule::Hide(p)));
    let mut sealed = all(&reach.denied.read);
    sealed.extend(all(&reach.denied.connect));
    let read_only = all(&reach.denied.write);
    let mut rules = denying(rules, &sealed, &read_only);
    rules.sort_by_key(Rule::key);
    let args = os::args(&rules, temp.is_some(), reach.network, own_pids);
    Ok(C::from_std(wrap(&program, &args, cmd), TOKEN))
}

fn denying<'a>(
    mut rules: Vec<Rule<'a>>,
    sealed: &'a [PathBuf],
    read_only: &'a [PathBuf],
) -> Vec<Rule<'a>> {
    for path in read_only {
        let reaching = rules
            .iter()
            .filter(|rule| path.starts_with(rule.path()))
            .max_by_key(|rule| rule.key())
            .copied();
        for rule in &mut rules {
            if let Rule::Write(below) = *rule {
                if below.starts_with(path) {
                    *rule = Rule::Read(below);
                }
            }
        }
        if let Some(Rule::Write(_)) = reaching {
            rules.push(Rule::Read(path));
        }
    }
    for path in sealed {
        rules.retain(|rule| !rule.path().starts_with(path));
        rules.push(Rule::Hide(path));
    }
    rules
}

pub fn host_problem(path: Option<&OsStr>) -> Option<NotSupported> {
    os::program(path).err()
}

fn path_of(cmd: &StdCommand) -> Option<OsString> {
    match cmd.get_envs().find(|(key, _)| *key == "PATH") {
        Some((_, value)) => value.map(OsString::from),
        None => std::env::var_os("PATH"),
    }
}

fn wrap(program: &Path, args: &[OsString], cmd: &StdCommand) -> StdCommand {
    let mut sandboxed = start::command(program).into_std(TOKEN);
    sandboxed
        .args(args)
        .arg(cmd.get_program())
        .args(cmd.get_args());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => sandboxed.env(key, value),
            None => sandboxed.env_remove(key),
        };
    }
    if let Some(dir) = cmd.get_current_dir() {
        sandboxed.current_dir(dir);
    }
    sandboxed
}

fn resolved(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if parent != path => resolved(parent).join(name),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::process::Command;
    use std::time::Duration;

    const OFF_HOST: &str = "192.0.2.1:9";
    const TARGET: &str = "SANDBOX_CHILD_TARGET";
    const INSIDE: &str = "inside";

    #[test]
    #[ignore = "run by the sandbox tests, inside a sandbox"]
    fn connect_child() {
        let target = std::env::var(TARGET).expect(TARGET);
        let listener = (target == INSIDE).then(|| TcpListener::bind("127.0.0.1:0").unwrap());
        let target = match &listener {
            Some(listener) => listener.local_addr().unwrap(),
            None => target.parse().unwrap(),
        };
        match TcpStream::connect_timeout(&target, Duration::from_secs(3)) {
            Ok(_) => println!("RESULT connected"),
            Err(e) => println!("RESULT refused {:?} {:?}", e.kind(), e.raw_os_error()),
        }
    }

    fn connect_in_local_sandbox(target: &str) -> Option<String> {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "sandbox::tests::connect_child", "--ignored"])
            .args(["--nocapture", "--test-threads=1"])
            .env(TARGET, target);
        let mut cmd = match sandboxed_local(cmd, &[], &Denials::default()) {
            Ok(cmd) => cmd,
            Err(_) => {
                assert!(
                    host_problem(None).is_some(),
                    "no sandbox on a host with one"
                );
                return None;
            }
        };
        let out = cmd.output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = text.split("RESULT ").nth(1).and_then(|r| r.lines().next());
        Some(
            line.unwrap_or_else(|| panic!("the child printed no result: {text}"))
                .to_string(),
        )
    }

    fn beat(path: &std::path::Path) -> Option<u64> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn beats_after(path: &std::path::Path, from: Option<u64>) -> bool {
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < until {
            if beat(path).is_some() && beat(path) != from {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn sandboxed_command_outlives_its_start_thread_and_ends_with_its_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let dir = resolved(dir.path());
        let heart = dir.join("beat");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("(i=0; while :; do i=$((i+1)); echo $i > beat; sleep 0.1; done) & wait")
            .current_dir(&dir);
        let Ok(cmd) = sandboxed_job(cmd, std::slice::from_ref(&dir), &Denials::default()) else {
            assert!(
                host_problem(None).is_some(),
                "no sandbox on a host with one"
            );
            return;
        };
        let mut child = std::thread::spawn(move || {
            let mut cmd = cmd;
            crate::group::new_group(&mut cmd);
            cmd.spawn().unwrap()
        })
        .join()
        .unwrap();
        assert!(beats_after(&heart, None), "the command started");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(child.try_wait().unwrap(), None, "the sandbox lives");
        assert!(beats_after(&heart, beat(&heart)), "the command lives");

        assert!(crate::group::kill_child_group(&mut child));
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let stopped = beat(&heart);
        assert!(
            !beats_after(&heart, stopped),
            "the heartbeat stops with the sandbox"
        );
    }

    #[test]
    fn local_network_allows_loopback_and_refuses_an_off_host_address() {
        let Some(line) = connect_in_local_sandbox(INSIDE) else {
            return;
        };
        assert_eq!(line, "connected", "a loopback listener in the sandbox");

        let webhook = TcpListener::bind("127.0.0.1:0").unwrap();
        let line = connect_in_local_sandbox(&webhook.local_addr().unwrap().to_string()).unwrap();
        if cfg!(target_os = "linux") {
            assert!(
                line.starts_with("refused ConnectionRefused"),
                "a listener of the host loopback: {line}"
            );
        } else {
            assert_eq!(line, "connected");
            assert!(webhook.accept().is_ok(), "the webhook got no connection");
        }

        let line = connect_in_local_sandbox(OFF_HOST).unwrap();
        assert!(line.starts_with("refused"), "{line}");
        assert!(
            !line.contains("TimedOut"),
            "a timeout is no refusal: {line}"
        );
        #[cfg(target_os = "macos")]
        assert!(
            line.contains("PermissionDenied"),
            "Seatbelt gives EPERM: {line}"
        );
    }
}
