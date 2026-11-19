//! Runs the check of a task in the worker sandbox (ticket 18; SR-4; F10).
//!
//! The check of a task runs code that a worker wrote, so it runs in the
//! sandbox profile of that worker and never with the rights of fleetd:
//!
//! - A Codex worker keeps `codex sandbox` with its `fleet_write` profile
//!   and the TMPDIR of its pane (F10, #181).
//! - Each other worker gets the check sandbox of the platform layer
//!   (`horch_platform::sandbox::sandboxed`). It can write only its worktree
//!   (the project root for a task of the shared checkout), the folder of
//!   its attempt and its temp folder, and it cannot do the 3 operator acts
//!   (SR-8).
//!
//! On a host with no platform sandbox, a persona of another CLI has no
//! check sandbox: fleetd does not dispatch it, and 1 needs-you item for
//! each persona says why (Q-A).
//!
//! A worker profile that profile validation refuses on this host (SR-8,
//! A37; `sandbox_rules.rs`) takes the same path: no dispatch and 1
//! needs-you item, unless its fallback has a sandbox here and takes the
//! task.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::sandbox::{sandboxed, Denials};
use horch_platform::NotSupported;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{NeedsYouItems, NewItem, SandboxProfiles};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::application::dispatch::{CheckSandboxes, NO_CHECK_SANDBOX};
use crate::fleetd::task_execution::domain::dispatch::Hold;
use crate::fleetd::worker_profiles::adapters::sandbox_rules;
use crate::fleetd::worker_profiles::application::ports::LaunchSpec;
use crate::fleetd::worker_profiles::domain::operator_boundary::OperatorPaths;
use crate::roster::Roster;
use crate::taskledger::finish::CodexSandbox;
use crate::taskledger::tasks;
use crate::taskledger::workers::Worker;

/// The kind of the needs-you item of a persona with no check sandbox. Its
/// key is `no_check_sandbox:<persona>`.
pub const NO_CHECK_SANDBOX_KIND: &str = "no_check_sandbox";

/// The check sandbox of each worker: Codex for a Codex worker, the
/// platform sandbox for each other worker.
#[derive(Debug, Clone)]
pub struct WorkerSandboxes {
    pub codex: CodexSandbox,
    /// The fleet root: the parent of each attempt folder and of the
    /// operator paths.
    pub fleet_root: PathBuf,
    /// Why this host has no platform sandbox, or `None` when it has one.
    pub gap: Option<NotSupported>,
    /// The worker profiles that profile validation refuses on this host,
    /// with the reason of each (SR-8, A37; `sandbox_rules.rs`).
    pub refused: BTreeMap<String, String>,
    /// The roster fallback of each persona. `None` for a persona with a
    /// `no_fallback_reason`: its task never moves to another persona.
    pub fallbacks: BTreeMap<String, Option<String>>,
}

impl WorkerSandboxes {
    /// The check sandboxes of the fleet at `fleet_root` on this host.
    pub fn new(codex: CodexSandbox, fleet_root: PathBuf) -> WorkerSandboxes {
        WorkerSandboxes {
            codex,
            fleet_root,
            gap: horch_platform::sandbox::host_problem(None),
            refused: BTreeMap::new(),
            fallbacks: BTreeMap::new(),
        }
    }

    /// These sandboxes with the profile refusals and the fallbacks of
    /// `roster`.
    pub fn with_profiles(mut self, roster: &Roster) -> WorkerSandboxes {
        self.refused = sandbox_rules::refused(roster, &self.fleet_root);
        self.fallbacks = roster
            .names()
            .into_iter()
            .filter_map(|name| {
                let t = roster.get(name)?;
                let fallback = match t.no_fallback_reason {
                    Some(_) => None,
                    None => t.profile().fallback,
                };
                Some((name.to_string(), fallback))
            })
            .collect();
        self
    }

    /// Give each task of `held` that waits for a persona with no sandbox
    /// here to its fallback, when the fallback has a sandbox here (the
    /// fallback rule of the roster: the fallback of the task, else the
    /// first fallback of the persona), in `tx`. Each moved task leaves
    /// `held`, so it opens no needs-you item, and the next dispatch pass
    /// binds it. Run it before [`WorkerSandboxes::open_items`].
    pub fn move_to_fallbacks(&self, tx: &Connection, held: &mut Vec<(String, Hold)>) -> Result<()> {
        let mut kept = Vec::with_capacity(held.len());
        for (task_id, hold) in held.drain(..) {
            let fallback = match &hold {
                Hold::Persona { persona, cause } if cause == NO_CHECK_SANDBOX => {
                    let task = tasks::get(tx, &task_id)?;
                    self.fallback_of(persona, task.fallback_persona.as_deref())
                        .map(|fb| (task, persona.clone(), fb))
                }
                _ => None,
            };
            let Some((mut task, persona, fb)) = fallback else {
                kept.push((task_id, hold));
                continue;
            };
            let why = self.why(&persona);
            task.fallback_persona = Some(fb);
            tasks::fallback(tx, &task, &format!("{persona}: {why}"))?;
        }
        *held = kept;
        Ok(())
    }

    /// The fallback of a task of `persona` whose own fallback is `own`,
    /// when it has a sandbox here. A persona with a `no_fallback_reason`
    /// has none.
    fn fallback_of(&self, persona: &str, own: Option<&str>) -> Option<String> {
        let fallback = match self.fallbacks.get(persona) {
            Some(None) => None,
            Some(Some(roster)) => Some(own.unwrap_or(roster)),
            None => own,
        };
        fallback
            .filter(|fb| *fb != persona && self.supported(fb))
            .map(str::to_string)
    }

    /// Why `persona` has no sandbox here.
    fn why(&self, persona: &str) -> String {
        if let Some(reason) = self.refused.get(persona) {
            return reason.clone();
        }
        self.gap.as_ref().map_or_else(
            || "the sandbox did not start".to_string(),
            |g| g.to_string(),
        )
    }

    /// Open 1 needs-you item for each persona of `held` that has the hold
    /// [`NO_CHECK_SANDBOX`], in `tx`. `held` is the list of held tasks of a
    /// dispatch pass. An open item of a persona stays 1 item.
    pub fn open_items(
        &self,
        tx: &Connection,
        needs_you: &dyn NeedsYouItems,
        held: &[(String, Hold)],
    ) -> Result<()> {
        let mut done: Vec<&str> = Vec::new();
        for (task_id, hold) in held {
            let Hold::Persona { persona, cause } = hold else {
                continue;
            };
            if cause != NO_CHECK_SANDBOX || done.contains(&persona.as_str()) {
                continue;
            }
            done.push(persona);
            let key = format!("{NO_CHECK_SANDBOX_KIND}:{persona}");
            let text = match self.refused.get(persona.as_str()) {
                Some(why) => format!(
                    "profile {persona} is {why}. fleetd dispatches no task of {persona}. \
                     Give the profile a sandbox that denies the 3 operator acts, or give its \
                     tasks another persona."
                ),
                None => format!(
                    "persona {persona} has {NO_CHECK_SANDBOX}: {}. fleetd dispatches no task \
                     of {persona}. Install the sandbox host, or give its tasks another persona.",
                    self.why(persona)
                ),
            };
            needs_you.open_item(
                tx,
                &NewItem {
                    key: &key,
                    kind: NO_CHECK_SANDBOX_KIND,
                    entity: Entity::Task,
                    entity_id: task_id,
                    text: &text,
                },
            )?;
        }
        Ok(())
    }

    /// The launch of `check` in `dir` in the platform sandbox, or `None`
    /// when this host has none.
    fn platform_launch(&self, worker: &Worker, dir: &Path, check: &str) -> Option<LaunchSpec> {
        if self.gap.is_some() {
            return None;
        }
        let mut writable = vec![dir.to_path_buf()];
        if let (Some(task_id), Some(n)) = (&worker.task_id, worker.attempt) {
            writable.push(attempt_dir(&self.fleet_root, task_id, &format!("A{n}")));
        }
        let paths = OperatorPaths::of(&self.fleet_root);
        let denied = Denials {
            read: vec![paths.token],
            connect: vec![paths.channel],
            write: vec![paths.run],
        };
        let mut cmd = std::process::Command::new(horch_platform::shell::CHECK_SHELL);
        cmd.arg("-c").arg(check);
        let cmd = sandboxed(cmd, &writable, &denied).ok()?;
        Some(LaunchSpec {
            program: cmd.get_program().into(),
            args: cmd.get_args().map(Into::into).collect(),
            cwd: Some(dir.to_path_buf()),
            env_rule: Default::default(),
            detach: false,
            output: None,
        })
    }
}

impl CheckSandboxes for WorkerSandboxes {
    fn supported(&self, persona: &str) -> bool {
        !self.refused.contains_key(persona)
            && (self.codex.personas.iter().any(|p| p == persona) || self.gap.is_none())
    }
}

impl SandboxProfiles for WorkerSandboxes {
    fn check_launch(&self, worker: &Worker, dir: &Path, check: &str) -> Result<Option<LaunchSpec>> {
        if self.codex.runs(worker) {
            return self.codex.check_launch(worker, dir, check);
        }
        Ok(self.platform_launch(worker, dir, check))
    }
}

/// The check of a Codex worker: the sandbox of its pane, by the rule of
/// `check_process` in `taskledger/finish/land.rs` (#181). Only the shell
/// gets the TMPDIR of the pane, as `/tmp` is read-only in the sandbox. A
/// worker of another CLI has no profile here.
impl SandboxProfiles for CodexSandbox {
    fn check_launch(&self, worker: &Worker, dir: &Path, check: &str) -> Result<Option<LaunchSpec>> {
        if !self.runs(worker) {
            return Ok(None);
        }
        let role = worker.role.as_deref().unwrap_or(&worker.worker_id);
        let tmpdir = crate::harness::codex::pane_tmpdir(&self.state_root, role);
        std::fs::create_dir_all(&tmpdir)
            .with_context(|| format!("creating {}", tmpdir.display()))?;
        let mut tmp_pair = std::ffi::OsString::from("TMPDIR=");
        tmp_pair.push(&tmpdir);
        let mut args: Vec<std::ffi::OsString> =
            crate::harness::codex::sandbox_args(&self.state_root, dir)
                .into_iter()
                .map(Into::into)
                .collect();
        args.extend([
            "/usr/bin/env".into(),
            tmp_pair,
            horch_platform::shell::CHECK_SHELL.into(),
            "-c".into(),
            check.into(),
        ]);
        Ok(Some(LaunchSpec {
            program: self.bin.clone(),
            args,
            cwd: Some(dir.to_path_buf()),
            env_rule: Default::default(),
            detach: false,
            output: None,
        }))
    }
}
