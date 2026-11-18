//! The launcher: it starts a worker or a job process.
//!
//! Each process of fleetd starts here (`docs/specs/fleetd/design.md`, SR-7),
//! through [`horch_platform::start::Command`]. The launcher removes the
//! forbidden keys with
//! [`scrub_child_env`](crate::runtime::process::scrub_child_env) and starts
//! the process as the leader of its own process group.
//!
//! Windows gap: Windows has no process groups, and the launcher makes no job
//! object, so a Windows child stays in the group of fleetd and its
//! descendants are not killed with it. A job object with kill-on-close does
//! not fit: it kills a job process when fleetd drops the handle.
//! `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, row
//! "Process groups".

use std::fs::{File, OpenOptions};
use std::process::{Child, Stdio};

use horch_platform::group;
use horch_platform::start::{self, Command};

use crate::fleetd::fleet_runtime::adapters::sqlite::in_door_transaction;
use crate::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use crate::runtime::process::{scrub_child_env, spawn_detached};

/// The [`ProcessHost`] of fleetd.
#[derive(Debug, Clone, Copy, Default)]
pub struct Launcher;

impl ProcessHost for Launcher {
    /// A detached process also loses the pane identity of its starter, as
    /// [`spawn_detached`] does today. A live process whose start time the
    /// OS does not give is killed, and the launch fails: fleetd cannot
    /// supervise it. A failed check of whether it exited does the same.
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        let inside = in_door_transaction();
        debug_assert!(!inside, "a process start inside a ledger transaction");
        if inside {
            return Err(LaunchError::InsideTransaction);
        }
        let mut cmd = command(spec)?;
        let mut child = if spec.detach {
            // A new session makes the child the leader of a new group too.
            spawn_detached(&mut cmd)?
        } else {
            scrub_child_env(&mut cmd);
            group::new_group(&mut cmd);
            cmd.spawn()?
        };
        let pid = child.id();
        let pid_start = horch_platform::process::start_time(pid);
        if pid_start.is_none() {
            let unreadable = match group::exited_without_reap(&mut child) {
                Ok(true) => None,
                Ok(false) => Some(std::io::Error::other(format!(
                    "the start time of process {pid} is unreadable"
                ))),
                Err(e) => Some(e),
            };
            if let Some(e) = unreadable {
                discard(child);
                return Err(LaunchError::Io(e));
            }
        }
        Ok(Launched {
            pid,
            pid_start,
            child,
        })
    }
}

/// Kill `child` and its group, then reap it. When no kill is sent, the
/// child is not reaped, so that the launcher never waits for a live
/// process.
fn discard(mut child: Child) {
    if group::kill_child_group(&mut child) || child.kill().is_ok() {
        let _ = child.wait();
    }
}

/// The command of `spec`, with the variables of its environment rule and
/// its output. The caller scrubs the environment after this.
fn command(spec: &LaunchSpec) -> Result<Command, LaunchError> {
    let mut cmd = start::command(&spec.program);
    cmd.args(&spec.args)
        .envs(spec.env_rule.vars.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    match &spec.output {
        Some(path) => {
            let file: File = OpenOptions::new().create(true).append(true).open(path)?;
            cmd.stdout(file.try_clone()?).stderr(file);
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::ideas::application::needs_you::open_item;
    use crate::fleetd::ideas::domain::needs_you::Write;
    use crate::fleetd::worker_profiles::application::ports::EnvRule;
    use crate::harness::launch::FORBIDDEN_ENV;
    use crate::taskledger::Store;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn spec(program: &str, args: &[&str]) -> LaunchSpec {
        LaunchSpec {
            program: PathBuf::from(program),
            args: args.iter().map(OsString::from).collect(),
            cwd: None,
            env_rule: EnvRule::default(),
            detach: false,
            output: None,
        }
    }

    /// Kill the group of `launched` and reap it.
    #[cfg(unix)]
    fn stop(mut launched: Launched) {
        horch_platform::group::kill_child_group(&mut launched.child);
        launched.child.wait().unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn launcher_removes_the_forbidden_key_for_a_process_with_no_profile() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("env.txt");
        for detach in [false, true] {
            let mut s = spec("/usr/bin/env", &[]);
            s.detach = detach;
            s.output = Some(out.clone());
            s.env_rule.vars = FORBIDDEN_ENV
                .iter()
                .map(|k| (OsString::from(k), OsString::from("from-the-caller")))
                .chain([("HORCH_LAUNCH_PROBE".into(), "kept".into())])
                .collect();
            let mut launched = Launcher.launch(&s).unwrap();
            assert!(launched.child.wait().unwrap().success());
            let seen = std::fs::read_to_string(&out).unwrap();
            std::fs::remove_file(&out).unwrap();
            assert!(
                seen.lines().any(|l| l == "HORCH_LAUNCH_PROBE=kept"),
                "detach {detach}: the caller's variable is missing:\n{seen}"
            );
            for key in FORBIDDEN_ENV {
                assert!(
                    !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
                    "detach {detach}: {key} reached the child"
                );
            }
        }
    }

    /// SR-7 through a real launch: this test binary runs again with each
    /// forbidden name in its environment, as fleetd can have it, and its
    /// launched children print their env.
    #[test]
    #[cfg(unix)]
    fn launcher_removes_a_forbidden_name_of_the_parent() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("env.txt");
        let mut parent = std::process::Command::new(std::env::current_exe().unwrap());
        parent
            .args([
                "--exact",
                "fleetd::worker_profiles::adapters::launcher::tests::launch_in_a_parent_with_a_forbidden_name",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HORCH_TEST_LAUNCH_ENV", &out);
        for key in FORBIDDEN_ENV {
            parent.env(key, "from-the-parent");
        }
        let run = parent.output().unwrap();
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        for detach in [false, true] {
            let seen = std::fs::read_to_string(out.with_extension(detach.to_string())).unwrap();
            assert!(
                seen.lines()
                    .any(|l| l.starts_with("HORCH_TEST_LAUNCH_ENV=")),
                "detach {detach}: the rest of the env is missing:\n{seen}"
            );
            for key in FORBIDDEN_ENV {
                assert!(
                    !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
                    "detach {detach}: {key} reached the child"
                );
            }
        }
    }

    #[test]
    #[cfg(unix)]
    #[ignore = "run by launcher_removes_a_forbidden_name_of_the_parent"]
    fn launch_in_a_parent_with_a_forbidden_name() {
        let out = PathBuf::from(
            std::env::var_os("HORCH_TEST_LAUNCH_ENV").expect("run by its parent test"),
        );
        for detach in [false, true] {
            let mut s = spec("/usr/bin/env", &[]);
            s.detach = detach;
            s.output = Some(out.with_extension(detach.to_string()));
            let mut launched = Launcher.launch(&s).unwrap();
            assert!(launched.child.wait().unwrap().success());
        }
    }

    #[test]
    #[cfg(unix)]
    fn launcher_starts_each_process_in_its_own_group() {
        for detach in [false, true] {
            let mut s = spec("/bin/sleep", &["30"]);
            s.detach = detach;
            let launched = Launcher.launch(&s).unwrap();
            let ps = std::process::Command::new("ps")
                .args(["-o", "pgid=", "-p", &launched.pid.to_string()])
                .output()
                .unwrap();
            let pgid = String::from_utf8_lossy(&ps.stdout).trim().to_string();
            let pid = launched.pid;
            stop(launched);
            assert_eq!(pgid, pid.to_string(), "detach {detach}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn launched_gives_the_pid_and_its_start_time() {
        let launched = Launcher.launch(&spec("/bin/sleep", &["30"])).unwrap();
        let pid = launched.pid;
        let start = launched.pid_start;
        let child_id = launched.child.id();
        let now = horch_platform::process::start_time(pid);
        stop(launched);
        assert_eq!(pid, child_id);
        assert!(start.is_some());
        assert_eq!(start, now);
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "a process start inside a ledger transaction")
    )]
    fn launch_inside_a_transaction_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        // A needs-you item is a door write that needs no run, task or idea.
        let item = Write {
            key: "test:1",
            kind: "test",
            entity: "job",
            entity_id: "1",
            text: "test",
        };
        let refused = store
            .immediate(|tx| {
                open_item(tx, &item)?;
                Ok(Launcher.launch(&spec("true", &[])))
            })
            .unwrap();
        assert!(
            matches!(refused, Err(LaunchError::InsideTransaction)),
            "{refused:?}"
        );
    }
}
