//! `horch spawn` - the port of `bin/horch-spawn`.
//!
//! Creates a new herdr pane running a worker agent, either as a FRESH session or
//! RESUMING a previous session id from the project ledger. Prints the new pane id
//! on stdout (human detail goes to stderr) so callers can chain splits when
//! building layouts. The rules live in `execution::plan` and the steps in
//! `execution::service`; this reads the inputs and prints.

use anyhow::{bail, Context, Result};
use horch_core::execution::plan::{self, GateInputs, MintedIds, PlanInputs};
use horch_core::execution::service::{ExecutionService, SpawnError};
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::{SessionMode, SpawnRequest, TilingMode};
use horch_core::ids::{ExecutionId, SessionId, TeammateName};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::Phase;
use horch_core::routing::decision::GateFlags;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Direction;

pub struct SpawnArgs {
    pub teammate: Option<String>,
    pub task: String,
    pub resume: Option<String>,
    pub phase: Option<Phase>,
    /// Effort for this spawn only, over the teammate file's (or the record's).
    pub effort: Option<String>,
    pub role: Option<String>,
    pub from_pane: Option<String>,
    pub direction: Direction,
    /// `Disabled` leaves the grid alone. `HORCH_TILE=0` says the same thing
    /// for every spawn.
    pub tiling: TilingMode,
    /// Never substitute a fallback teammate (the usage-limit gate).
    pub exact: bool,
    /// Never refuse over usage limits.
    pub force: bool,
}

/// Split `horch spawn`'s positional arguments into a teammate and a task.
///
/// The two documented forms take positionals in different slots:
///   `horch spawn <teammate> ["task"]`
///   `horch spawn --resume <id> ["task"]`
/// With `--resume` the teammate comes from the ledger record, so the lone
/// positional is the task. Letting the parser bind positionally instead would
/// reject `--resume s-1 "continue"` with "unknown teammate 'continue'".
pub fn resolve_positionals(
    positionals: &[String],
    resume: Option<&str>,
) -> Result<(Option<String>, String)> {
    if resume.is_some() {
        return match positionals {
            [] => Ok((None, String::new())),
            [task] => Ok((None, task.clone())),
            _ => bail!("--resume takes at most one positional argument: the task"),
        };
    }
    match positionals {
        [] => bail!("give a teammate (e.g. `horch spawn sonnet \"task\"`) or --resume <id>"),
        [name] => Ok((Some(name.clone()), String::new())),
        [name, task] => Ok((Some(name.clone()), task.clone())),
        _ => bail!("too many arguments; usage: horch spawn <teammate> [\"task\"]"),
    }
}

/// Spawn a worker pane, returning its pane id so callers can chain splits.
/// A refusal is a [`SpawnError::Refused`]; its REFUSED line is on stdout.
pub fn spawn(ctx: &RuntimeContext, args: SpawnArgs) -> Result<String> {
    let mut req = SpawnRequest::worker(
        args.teammate
            .as_deref()
            .map(TeammateName::new)
            .transpose()?,
        args.task,
    );
    if req.teammate.is_none() && args.resume.is_none() {
        return Err(SpawnError::from(plan::PlanError::NothingToSpawn).into());
    }
    (req.resume, req.phase, req.effort) = (args.resume, args.phase, args.effort);
    (req.from_pane, req.direction, req.tiling) = (args.from_pane, args.direction, args.tiling);
    req.flags = GateFlags {
        exact: args.exact,
        force: args.force,
    };

    let roster_dir = super::path_text(ctx.bins.roster_override.as_deref());
    let roster = super::load_roster(ctx, roster_dir.as_deref())?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
        .context("horch spawn needs HORCH_WORKSPACE_ID set, or to run inside a herdr pane")?;
    std::fs::create_dir_all(mailbox.dir())?;
    // Read once, so the ledger this process writes and the one the worker
    // later reads (its brief carries the project) are the same file.
    let project = ctx.paths.project()?;
    let store = ExecutionStore::open_in(ctx)?;

    let existing = req.resume.as_deref().and_then(|key| store.get(key).ok());
    if let Some(r) = existing.as_ref().filter(|r| r.round_id.is_some()) {
        anyhow::bail!(
            "record {} is a competition {} (round {}); multi-herdr-dataset owns it, \
             so horch spawn --resume refuses it",
            r.record_id,
            if r.experiment_id.is_some() {
                "candidate"
            } else {
                "judge"
            },
            r.round_id.as_deref().unwrap_or_default()
        );
    }
    let root = &ctx.paths.state_root;
    let gated = match plan::needs_gate(&req, &roster) {
        true => {
            let policy = super::quotacmd::load_policy(ctx, root)?;
            let env = super::quotacmd::quota_env(ctx);
            let now = horch_core::clock::now();
            let view = horch_core::routing::snapshot::obtain(root, now, &policy, true, &env)?;
            Some((view, policy.balance_mode))
        }
        false => None,
    };
    let ids = MintedIds {
        execution: ExecutionId::new(horch_core::mint_uuid())?,
        session: SessionId::new(horch_core::mint_uuid())?,
    };
    let catalog = roster.skill_catalog()?;
    let inputs = PlanInputs {
        roster: &roster,
        catalog: &catalog,
        gate: gated.as_ref().map(|(view, balance)| GateInputs {
            view,
            balance: *balance,
        }),
        existing: existing.as_ref(),
        now: horch_core::clock::now(),
        ids: &ids,
        project: &project,
    };
    let plan = match plan::plan_launch(&req, &inputs).map_err(SpawnError::from) {
        Err(e @ SpawnError::Refused { .. }) => {
            if let SpawnError::Refused { line, .. } = &e {
                crate::output::println(line);
            }
            return Err(e.into());
        }
        other => other?,
    };
    if let Some(line) = &plan.gate_line {
        crate::output::println(line);
    }

    // Lay the grid out, which also puts the pane where it belongs: a split
    // halves its parent, and the tiler moves the worker into the next free
    // slot and evens every column. Best effort: a grid that will not lay out
    // must not fail a spawn whose worker is already running.
    let tile = |pane: &str, tiling: TilingMode| {
        if tiling == TilingMode::Disabled {
            if let Ok(layout) = herdr.pane_layout(Some(pane)) {
                arrange::equalize_quietly(ctx, &herdr, layout);
            }
        } else {
            arrange::after_change(ctx, &herdr, mailbox.workspace_id(), Some(pane));
        }
    };
    let service = ExecutionService {
        ctx,
        store: &store,
        workspace: &herdr,
        mailbox: &mailbox,
        tile: &tile,
    };
    let out = service.spawn(plan, args.role.as_deref())?;
    let (launch, role, pane) = (&out.plan.launch, &out.plan.execution.role, &out.pane);
    let what = format!("{} {}", launch.teammate.agent, launch.model);
    match &launch.session {
        SessionMode::Resume(id) => {
            eprintln!("spawned {role} (pane {pane}): {what}, RESUMING session {id}")
        }
        SessionMode::Fresh(None) => eprintln!("spawned {role} (pane {pane}): {what}, new session"),
        SessionMode::Fresh(Some(id)) => {
            eprintln!("spawned {role} (pane {pane}): {what}, new session {id}")
        }
    }
    Ok(out.pane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::roster::Roster;

    #[test]
    fn a_bare_teammate_spawns_an_idle_worker() {
        let (name, task) = resolve_positionals(&["sonnet".into()], None).unwrap();
        assert_eq!(name.as_deref(), Some("sonnet"));
        assert_eq!(task, "");
    }

    #[test]
    fn a_teammate_and_task_are_taken_in_order() {
        let (name, task) =
            resolve_positionals(&["codex-sol".into(), "fix auth".into()], None).unwrap();
        assert_eq!(name.as_deref(), Some("codex-sol"));
        assert_eq!(task, "fix auth");
    }

    /// The regression this resolver exists for: with --resume, the lone
    /// positional is the task, not a teammate.
    #[test]
    fn resume_takes_its_positional_as_the_task() {
        let (name, task) =
            resolve_positionals(&["continue the refactor".into()], Some("s-1")).unwrap();
        assert!(name.is_none(), "teammate must come from the ledger record");
        assert_eq!(task, "continue the refactor");
    }

    #[test]
    fn resume_without_a_task_is_allowed() {
        let (name, task) = resolve_positionals(&[], Some("s-1")).unwrap();
        assert!(name.is_none());
        assert_eq!(task, "");
    }

    #[test]
    fn no_arguments_at_all_explains_both_forms() {
        let err = resolve_positionals(&[], None).unwrap_err().to_string();
        assert!(err.contains("horch spawn sonnet"), "{err}");
        assert!(err.contains("--resume"), "{err}");
    }

    /// Teammate names come from a directory, not a closed enum, so argv parsing
    /// accepts any word and the roster is what rejects it - with a list of what
    /// actually exists on this machine, which a compiled-in enum could not give.
    #[test]
    fn an_unknown_teammate_lists_the_valid_ones() {
        let (name, _) = resolve_positionals(&["haiku".into()], None).unwrap();
        let err = Roster::builtin()
            .unwrap()
            .require(name.as_deref().unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown teammate 'haiku'"), "{err}");
        assert!(err.contains("codex-terra"), "{err}");
    }

    #[test]
    fn extra_positionals_are_rejected_rather_than_silently_dropped() {
        assert!(
            resolve_positionals(&["sonnet".into(), "task".into(), "extra".into()], None).is_err()
        );
        assert!(resolve_positionals(&["task".into(), "extra".into()], Some("s-1")).is_err());
    }
}
