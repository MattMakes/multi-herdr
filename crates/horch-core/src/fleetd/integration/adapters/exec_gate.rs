//! The executor of the `fast_gate` and `full_gate` jobs (ticket 28).
//!
//! `docs/specs/fleetd/design.md`, "Between features" (Two declared gates,
//! The full gate) and "The kinds of job"; `docs/tickets/fleetd-p4/MAP.md`,
//! Q18, Q19, Q24 and Q26.
//!
//! A `fast_gate` run judges the candidate tip of a bundle over the feature
//! tip that the candidate is built on. A `full_gate` run judges the feature
//! tip over the fetched tip of the primary branch: the feature tip must
//! contain that tip, else the gate waits for the rebase. Both are pure: the
//! input key is the 2 tips ([`gate_key`]).
//!
//! The run reads the gate command from the project `fleet.toml` at the tip
//! that it judges, runs `git checkout --detach <tip>` in the gate tree of
//! the feature by the fleetd git rule, then runs the command there through
//! the launcher, in the gate sandbox. A gate runs code that workers wrote,
//! so it never runs with the rights of fleetd. It can write only the gate
//! tree, its temp folder and the gate cache of the feature; the project
//! adds no path. The exit code is the verdict, written in `gate_runs`.
//! With no sandbox the gate does not run, and the feature holds with 1
//! needs-you item until a pass finds a sandbox ([`end_hold`]).
//!
//! The time limit is the deadline of the job: the pass stops the job
//! process, and the job process kills the group of its gate first.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::sandbox::{sandboxed_job, Denials};
use horch_platform::shell::CHECK_SHELL;
use horch_platform::start::{self, Command};
use horch_platform::NotSupported;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, NewItem, Queued, Ran};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, Outcome};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::open_keys;
use crate::fleetd::integration::adapters::executors::{check_text, clip, facts_of, FACT_TEXT_MAX};
use crate::fleetd::integration::adapters::git::CliGitGateway;
use crate::fleetd::integration::adapters::project_file;
use crate::fleetd::integration::adapters::sqlite::{self as rows, NewGateRun};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::domain::project::FILE_NAME;
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec, ProcessHost};
use crate::fleetd::worktree_custody::adapters::executors::fleet_root;
use crate::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use crate::fleetd::worktree_custody::application::gate_tree;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;
use crate::taskledger::model::Actor;

/// The start of the cause of a run on a host with no gate sandbox.
pub const NO_GATE_SANDBOX: &str = "no gate sandbox";

/// The needs-you kind of a feature whose gates hold for want of a sandbox.
pub const NO_GATE_SANDBOX_KIND: &str = "no_gate_sandbox";

/// The needs-you key of the hold of `feature_id`.
pub fn no_gate_sandbox_key(feature_id: &str) -> String {
    format!("{NO_GATE_SANDBOX_KIND}:{feature_id}")
}

/// The gate sandbox: `cmd` that can write only below `writable` and in its
/// temp folder, with the operator acts `denied`
/// ([`horch_platform::sandbox::sandboxed_job`]). A test gives a host with no
/// sandbox.
pub type GateSandbox = fn(Command, &[PathBuf], &Denials) -> Result<Command, NotSupported>;

/// The input key of a gate run: the tip that it judges and the tip that it
/// judges it over.
pub fn gate_key(tip: &str, base_tip: &str) -> String {
    serde_json::json!([tip, base_tip]).to_string()
}

fn read_key(key: &str) -> Option<(String, String)> {
    serde_json::from_str(key).ok()
}

/// The word of the gate of `kind` in `gate_runs`, and its key in the
/// project file.
fn gate_word(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::FastGate => Some("fast"),
        Kind::FullGate => Some("full"),
        _ => None,
    }
}

/// 1 gate to queue: `kind` of `subject` (the bundle for `fast_gate`, the
/// feature for `full_gate`) of the feature `feature_id`, on `tip` over
/// `base_tip`, in the repository `repo`.
#[derive(Debug, Clone, Copy)]
pub struct GateRequest<'a> {
    pub kind: Kind,
    pub subject: &'a str,
    pub feature_id: &'a str,
    pub tip: &'a str,
    pub base_tip: &'a str,
    pub repo: &'a Path,
}

/// Queue the gate `req`, in `tx`, at `clock`: the job run, and for a new
/// run its `gate_runs` row with no result (row first). `None` while the
/// feature holds for want of a gate sandbox: no gate of it is queued.
pub fn queue(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    req: &GateRequest<'_>,
    clock: FleetSeconds,
) -> Result<Option<Queued>> {
    let Some(gate) = gate_word(req.kind) else {
        bail!("{} is not a gate kind", req.kind);
    };
    if held(tx, req.feature_id)? {
        return Ok(None);
    }
    let input_key = gate_key(req.tip, req.base_tip);
    let key = JobKey {
        kind: req.kind,
        subject: req.subject,
        input_key: &input_key,
    };
    let queued = jobs::queue(ctx, tx, &key, Some(&req.repo.to_string_lossy()), clock)?;
    if let Queued::New(job_id) = queued {
        let new = NewGateRun {
            feature_id: req.feature_id,
            gate,
            tip: req.tip,
            base_tip: req.base_tip,
            job_id,
        };
        rows::insert_gate_run(tx, ctx.guard, &new, by_fleetd("the gate is queued"))?;
    }
    Ok(Some(queued))
}

/// Whether the gates of `feature_id` hold for want of a gate sandbox: its
/// needs-you item is open.
pub fn held(conn: &Connection, feature_id: &str) -> Result<bool> {
    let key = no_gate_sandbox_key(feature_id);
    Ok(open_keys(conn, NO_GATE_SANDBOX_KIND)?.contains(&key))
}

/// Why this host has no gate sandbox `sandbox`, or `None` when it has one.
/// A pass calls it before [`end_hold`].
pub fn sandbox_gap(sandbox: GateSandbox) -> Option<NotSupported> {
    sandbox(start::command(CHECK_SHELL), &[], &Denials::default()).err()
}

/// End the hold of `feature_id`, in `tx`: close its needs-you item. `false`
/// when it did not hold.
pub fn end_hold(ctx: &JobCtx<'_>, tx: &Connection, feature_id: &str) -> Result<bool> {
    ctx.needs_you
        .resolve_item(tx, &no_gate_sandbox_key(feature_id))
}

/// Runs the job `row` in the gate sandbox of this host.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    run_in(conn, row, sandboxed_job)
}

/// [`run`] with the gate sandbox `sandbox`. It writes no row: the
/// verdict, or the hold of a host with no gate sandbox, is in its result,
/// and [`record`] in fleetd writes it.
pub fn run_in(conn: &Connection, row: &JobRow, sandbox: GateSandbox) -> Result<Ran> {
    let Some(gate) = gate_word(row.kind) else {
        return Ok(no_verdict(format!("{} is not a gate kind", row.kind)).into());
    };
    let Some((tip, base_tip)) = read_key(&row.input_key) else {
        return Ok(no_verdict(format!(
            "the {} job {} has no input key of 2 tips: {}",
            row.kind, row.id, row.input_key
        ))
        .into());
    };
    let Some(repo) = row.repo.as_deref().map(PathBuf::from) else {
        return Ok(no_verdict(format!("the {} job {} has no repository", row.kind, row.id)).into());
    };
    let feature = match feature_of(conn, row)? {
        Ok(feature) => feature,
        Err(why) => return Ok(no_verdict(why).into()),
    };
    let run = GateRun {
        gate,
        feature,
        tip,
        base_tip,
        repo,
        root: fleet_root(conn)?,
    };
    run.go(conn, row, sandbox)
}

/// The facts of a gate run in its result file: the cause of the hold of a
/// host with no gate sandbox. A run with a verdict has no facts.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateFacts {
    held: Option<String>,
}

/// Record the result `ran` of the gate run `row`, in `tx` of fleetd: the
/// verdict in its `gate_runs` row, or the needs-you item that holds the
/// feature. The gate, the tips and the feature come from the row, so a
/// file changes only the gate run of its own job.
pub fn record(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, ran: &Ran) -> Result<()> {
    let facts: GateFacts = facts_of(ran.data.as_ref())?.unwrap_or_default();
    if facts.held.is_none() && !ran.outcome.verdict {
        return Ok(());
    }
    let gate = gate_word(row.kind).with_context(|| format!("{} is not a gate kind", row.kind))?;
    let feature = feature_of(tx, row)?.map_err(|why| anyhow!(why))?;
    if let Some(gap) = facts.held {
        if ran.outcome.verdict {
            bail!("a held gate has no verdict");
        }
        check_text("held", &gap, FACT_TEXT_MAX)?;
        return hold(ctx, tx, &feature, &gap);
    }
    let (tip, base_tip) = read_key(&row.input_key)
        .with_context(|| format!("the input key {} has no 2 tips", row.input_key))?;
    if let Some(run) = rows::gate_run_of(tx, &feature, gate, &tip, &base_tip)? {
        let result = if ran.outcome.passed { "pass" } else { "fail" };
        rows::set_gate_result(
            tx,
            ctx.guard,
            run.id,
            row.id,
            result,
            by_fleetd("the gate ran"),
        )?;
    }
    Ok(())
}

/// The gate of `feature` does not run for the cause `cause`: 1 needs-you
/// item holds the feature.
fn hold(ctx: &JobCtx<'_>, tx: &Connection, feature: &str, cause: &str) -> Result<()> {
    let key = no_gate_sandbox_key(feature);
    let text = format!(
        "{cause}. The gates of feature {feature} do not run, and none of its bundles gates \
         or lands, until this host has a sandbox."
    );
    let item = NewItem {
        key: &key,
        kind: NO_GATE_SANDBOX_KIND,
        entity: Entity::Feature,
        entity_id: feature,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

/// The feature of the gate run `row`: of its bundle for a fast gate, its
/// subject for a full gate. The inner `Err` is why it has none.
fn feature_of(conn: &Connection, row: &JobRow) -> Result<Result<String, String>> {
    if row.kind == Kind::FullGate {
        return Ok(Ok(row.subject.clone()));
    }
    let Ok(id) = row.subject.parse::<i64>() else {
        return Ok(Err(format!(
            "the fast_gate subject {} is not a bundle",
            row.subject
        )));
    };
    Ok(match rows::bundle(conn, id)? {
        Some(bundle) => Ok(bundle.feature_id),
        None => Err(format!("no bundle {id} in the ledger")),
    })
}

/// The facts of 1 gate run.
struct GateRun {
    gate: &'static str,
    feature: String,
    tip: String,
    base_tip: String,
    repo: PathBuf,
    root: PathBuf,
}

impl GateRun {
    fn go(&self, conn: &Connection, row: &JobRow, sandbox: GateSandbox) -> Result<Ran> {
        let tree = match gate_tree::of_feature(conn, &self.repo, &self.feature)? {
            Some(t) if t.state == WorktreeState::Created && Path::new(&t.path).is_dir() => {
                PathBuf::from(t.path)
            }
            _ => {
                return Ok(
                    no_verdict(format!("the gate tree of {} is not made yet", self.feature)).into(),
                )
            }
        };
        let cache = gate_tree::cache_dir(&self.root, &self.feature);
        let writable = [tree.clone(), cache.clone()];
        let denied = operator_denials(&self.root);
        let git_dir = match repo_git_dir(&self.repo) {
            Ok(dir) => dir,
            Err(e) => return Ok(no_verdict(format!("{e:#}")).into()),
        };
        let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)), &self.root)
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[git_dir, tree.clone()]);
        let probe = sandbox(start::command(CHECK_SHELL), &writable, &denied);
        if let Some(gap) = probe.err().or_else(|| git.sandbox_gap()) {
            return Ok(no_sandbox(&gap));
        }
        if self.gate == "full" && !self.contains_base(&git)? {
            return Ok(no_verdict(format!(
                "the feature tip {} does not contain the fetched primary tip {}; \
                 the full gate waits for the rebase",
                self.tip, self.base_tip
            ))
            .into());
        }
        let command = match self.command(&git) {
            Ok(Some(command)) => command,
            Ok(None) => {
                return Ok(no_verdict(format!(
                    "no {gate} gate: add [gates] {gate} to {FILE_NAME} at {tip}",
                    gate = self.gate,
                    tip = self.tip
                ))
                .into())
            }
            Err(e) => return Ok(no_verdict(format!("{e:#}")).into()),
        };
        if let Err(e) = git.git(&tree, &["checkout", "--force", "--detach", &self.tip]) {
            return Ok(no_verdict(format!("{e:#}")).into());
        }
        let spec = match self.spec(row, &command, &tree, &cache, sandbox, &denied)? {
            Ok(spec) => spec,
            Err(gap) => return Ok(no_sandbox(&gap)),
        };
        let status = match run_gate(&Launcher, &spec) {
            Ok(status) => status,
            Err(e) => return Ok(no_verdict(format!("`{command}` did not start: {e:#}")).into()),
        };
        let code = status
            .code()
            .map_or_else(|| "a signal".to_string(), |c| c.to_string());
        Ok(Ran::from(Outcome {
            passed: status.success(),
            verdict: true,
            text: format!(
                "`{command}` on {} in {}: exit {code}; output in {}",
                self.tip,
                tree.display(),
                spec.output.as_deref().unwrap_or(Path::new("")).display()
            ),
        }))
    }

    /// Whether the feature tip contains the fetched primary tip.
    fn contains_base(&self, git: &CliWorktreeGit) -> Result<bool> {
        let args = ["merge-base", "--is-ancestor", &self.base_tip, &self.tip];
        let out = git.checked(&self.repo, &args)?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            _ => bail!(
                "git merge-base --is-ancestor failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    /// The declared command of the gate at the tip that it judges.
    fn command(&self, git: &CliWorktreeGit) -> Result<Option<String>> {
        let text = CliGitGateway::new(git.clone()).fleet_toml(&self.repo, &self.tip)?;
        let Some(text) = text else {
            return Ok(None);
        };
        let settings = project_file::parse(&text).with_context(|| format!("at {}", self.tip))?;
        let command = if self.gate == "fast" {
            settings.fast_gate()
        } else {
            settings.full_gate()
        };
        Ok(command.map(str::to_string))
    }

    /// The launch of `command` in `tree`, in the gate sandbox, with the
    /// gate cache. The inner `Err` is the sandbox gap.
    fn spec(
        &self,
        row: &JobRow,
        command: &str,
        tree: &Path,
        cache: &Path,
        sandbox: GateSandbox,
        denied: &Denials,
    ) -> Result<Result<LaunchSpec, NotSupported>> {
        let cargo = cache.join("cargo");
        let xdg = cache.join("xdg");
        for dir in [&cargo, &xdg] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let output = self
            .root
            .join("gate-output")
            .join(format!("{}.log", row.id));
        if let Some(dir) = output.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut cmd = start::command(CHECK_SHELL);
        cmd.arg("-c")
            .arg(command)
            .current_dir(tree)
            .env("CARGO_HOME", &cargo)
            .env("XDG_CACHE_HOME", &xdg);
        let writable = [tree.to_path_buf(), cache.to_path_buf()];
        let cmd = match sandbox(cmd, &writable, denied) {
            Ok(cmd) => cmd,
            Err(gap) => return Ok(Err(gap)),
        };
        Ok(Ok(LaunchSpec {
            program: cmd.get_program().into(),
            args: cmd.get_args().map(Into::into).collect(),
            cwd: cmd.get_current_dir().map(Path::to_path_buf),
            env_rule: EnvRule {
                vars: cmd
                    .get_envs()
                    .filter_map(|(k, v)| Some((k.into(), v?.into())))
                    .collect(),
            },
            detach: false,
            output: Some(output),
        }))
    }
}

/// The result of a gate that does not run, because this host has no gate
/// sandbox for the cause `gap`: no verdict, and the hold in its facts.
fn no_sandbox(gap: &NotSupported) -> Ran {
    let cause = clip(format!("{NO_GATE_SANDBOX}: {gap}"));
    let facts = GateFacts {
        held: Some(cause.clone()),
    };
    Ran {
        outcome: no_verdict(cause),
        data: Some(serde_json::to_value(facts).expect("the facts serialize")),
    }
}

/// The operator acts that a gate cannot do, as a worker cannot (SR-8):
/// read the operator token, connect to the command channel, write a
/// request file.
fn operator_denials(root: &Path) -> Denials {
    let run = root.join("run");
    Denials {
        read: vec![run.join("operator.token")],
        connect: vec![run.join("fleetd.sock")],
        write: vec![run.join("requests")],
    }
}

/// The group of the running gate, 0 when none runs.
static GATE_GROUP: AtomicI32 = AtomicI32::new(0);

/// Start `spec` through `host` and wait for it. The gate leads its own
/// group. A stop of this job process at its deadline kills that group
/// first, so no gate outlives its job. A stop between the launch and the
/// store of the group waits in the hold until the group is stored
/// (T-369). Windows has no signal handlers: there the gate lives on after
/// the stop (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
/// section 6, row "Signals and stop").
fn run_gate(host: &dyn ProcessHost, spec: &LaunchSpec) -> Result<ExitStatus> {
    let _ = horch_platform::signal::kill_group_on_termination(&GATE_GROUP);
    let mut launched = {
        let _hold = horch_platform::signal::hold_termination();
        let launched = host.launch(spec)?;
        GATE_GROUP.store(i32::try_from(launched.pid).unwrap_or(0), Ordering::SeqCst);
        launched
    };
    let status = launched.child.wait();
    GATE_GROUP.store(0, Ordering::SeqCst);
    Ok(status?)
}

fn no_verdict(text: String) -> Outcome {
    Outcome {
        passed: false,
        verdict: false,
        text,
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::application::jobs::stop_window;

    /// A `SIGTERM` of the job process between the launch of the gate and
    /// the store of its group ends the gate group (T-369, F4).
    #[test]
    fn a_stop_between_the_launch_and_the_store_of_the_gate_ends_its_group() {
        stop_window::prove(
            "fleetd::integration::adapters::exec_gate::tests::a_stop_between_the_launch_and_the_store_of_the_gate_ends_its_group",
            |host, spec| {
                let _ = run_gate(host, spec);
            },
        );
    }
}
