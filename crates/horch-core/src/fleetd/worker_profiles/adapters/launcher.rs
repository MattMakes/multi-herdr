//! The launcher: it starts a worker or a job process.
//!
//! Each process of fleetd starts here (`docs/specs/fleetd/design.md`, SR-7).
//! The launcher removes the forbidden keys with
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
use std::process::{Command, Stdio};

use horch_platform::group;

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
    /// supervise it.
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
        if pid_start.is_none() && !group::exited_without_reap(&mut child)? {
            group::kill_child_group(&mut child);
            let _ = child.wait();
            return Err(LaunchError::Io(std::io::Error::other(format!(
                "the start time of process {pid} is unreadable"
            ))));
        }
        Ok(Launched {
            pid,
            pid_start,
            child,
        })
    }
}

/// The command of `spec`, with the variables of its environment rule and
/// its output. The caller scrubs the environment after this.
fn command(spec: &LaunchSpec) -> Result<Command, LaunchError> {
    let mut cmd = Command::new(&spec.program);
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
    use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
    use crate::fleetd::fleet_runtime::domain::ledger::Entity;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::fleetd::worker_profiles::application::ports::EnvRule;
    use crate::harness::launch::FORBIDDEN_ENV;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
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
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, dir.path(), &new))
            .unwrap();
        let new = NewTask::work("t1", "t1.md", "dev");
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let cause = Cause {
            actor: Actor::Horch,
            text: "test",
        };
        let refused = store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Task,
                    &task,
                    "cancelled",
                    cause,
                    &NeedsYouList,
                )?;
                Ok(Launcher.launch(&spec("true", &[])))
            })
            .unwrap();
        assert!(
            matches!(refused, Err(LaunchError::InsideTransaction)),
            "{refused:?}"
        );
    }
}
