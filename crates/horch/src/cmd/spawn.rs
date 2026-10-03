//! `horch spawn` - the port of `bin/horch-spawn`.
//!
//! Creates a new herdr pane running a worker agent, either as a FRESH session or
//! RESUMING a previous session id from the project ledger. Prints the new pane id
//! on stdout (human detail goes to stderr) so callers can chain splits when
//! building layouts.

use anyhow::{bail, Context, Result};
use horch_core::balance_policy;
use horch_core::execution::{SessionMode, TilingMode};
use horch_core::herdr::{Direction, Herdr};
use horch_core::ids::SessionId;
use horch_core::ledger::{Ledger, Record, STATUS_WORKING};
use horch_core::mailbox::{Brief, Mailbox};
use horch_core::paneshell::PaneShell;
use horch_core::routing::decision::{self, Decision, GateFlags, RoutingMode, RoutingProvenance};
use horch_core::runtime::RuntimeContext;
use horch_core::teammates::{effort_problem, Phase, Roster, Teammate};

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

/// The gate refused the spawn. `main` turns this into exit code 3; the
/// REFUSED line is already on stdout.
#[derive(Debug)]
pub struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("spawn refused over usage limits")
    }
}

impl std::error::Error for Refused {}

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
pub fn spawn(ctx: &RuntimeContext, args: SpawnArgs) -> Result<String> {
    if args.teammate.is_none() && args.resume.is_none() {
        bail!("give a teammate (e.g. `horch spawn sonnet \"task\"`) or --resume <id>");
    }
    let roster_dir = super::path_text(ctx.bins.roster_override.as_deref());
    let roster = super::load_roster(ctx, roster_dir.as_deref())?;

    let herdr = Herdr::new();
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
        .context("horch spawn needs HORCH_WORKSPACE_ID set, or to run inside a herdr pane")?;
    std::fs::create_dir_all(mailbox.dir())?;

    // The project dir is read once, so the ledger this process writes and the
    // ledger the worker later reads (its brief carries it) are the same file.
    let project_dir = ctx.paths.project()?.to_string_lossy().into_owned();

    let ledger = Ledger::open_in(ctx)?;
    let mut plan = match &args.resume {
        // Resuming: teammate, model and session all come from the record.
        Some(key) => {
            let record = ledger.get(key)?;
            if record.is_orchestrator() {
                bail!(
                    "record {} is an orchestrator; restart it with horch fleet",
                    record.record_id
                );
            }
            let session_id = record.session_id.clone().unwrap_or_default();
            if session_id.is_empty() {
                bail!(
                    "record {} has no captured session id; spawn a fresh {} instead",
                    record.record_id,
                    record.tier
                );
            }
            if record.status == STATUS_WORKING {
                bail!(
                    "session {session_id} is still marked working (a live worker may own it); \
                     refusing to resume"
                );
            }
            let mut teammate = roster.require(&record.tier)?.clone();
            // A substituted session resumes on the harness it ran on (BAL-05):
            // the same fallback's launch settings, never a new gate decision.
            if let Some(via) = &record.via {
                teammate = balance_policy::merge(&teammate, roster.require(via)?);
            }
            teammate.phase = resolve_phase(args.phase, record.phase, teammate.phase);
            // A resume keeps the level it ran at, as it keeps its model.
            if record.effort.is_some() {
                teammate.effort = record.effort.clone();
            }
            Roster::is_spawnable(&teammate)?;
            // A resume keeps the routing of the record it resumes. A record
            // from before A5 has none; rebuild it from `via`.
            let routing = match &record.routing {
                Some(r) => r.clone(),
                None => RoutingProvenance::legacy(
                    &record.tier,
                    record.via.as_deref(),
                    &record.agent,
                    &record.model,
                    record.substitution_reason.as_deref(),
                    roster.get(&record.tier),
                )?,
            }
            .resumed();
            Plan {
                teammate,
                model: record.model.clone(),
                record_id: record.record_id.clone(),
                session: SessionMode::Resume(SessionId::new(session_id)?),
                via: record.via.clone(),
                substitution_reason: record.substitution_reason.clone(),
                routing,
            }
        }
        None => {
            let name = args.teammate.clone().expect("checked above");
            let mut teammate = roster.require(&name)?.clone();
            teammate.phase = resolve_phase(args.phase, None, teammate.phase);
            Roster::is_spawnable(&teammate)?;
            Plan {
                routing: RoutingProvenance::ungated(&teammate)?,
                model: teammate.model.clone().unwrap_or_default(),
                record_id: horch_core::mint_uuid(),
                // Claude, pi and Prime accept a caller-minted session id, so
                // the ledger knows the resume handle before the agent even
                // starts. Codex and OpenCode reveal theirs only after launch;
                // `horch worker` harvests those into the ledger asynchronously.
                session: fresh_session(teammate.agent)?,
                teammate,
                via: None,
                substitution_reason: None,
            }
        }
    };

    // The last gate before launch, and the only one that sees what will ACTUALLY
    // start: `is_spawnable` above reads the teammate FILE's model, but a resume
    // takes its model from the ledger record, so a stale or hand-edited record
    // could otherwise start a second top-tier session behind an ordinary tier
    // name. Both branches pass through here.
    Roster::model_is_spawnable(&plan.model, &plan.teammate.name)?;

    // The usage-limit gate (design 13.5): after the top-tier gate, before
    // any role, ledger or pane side effect. A resume keeps the harness it
    // ran on, so only a fresh spawn is gated; the smoke fake spends nothing.
    if !plan.session.is_resume() && plan.teammate.agent != horch_core::teammates::Agent::None {
        let root = ctx.paths.state_root.clone();
        let policy = super::quotacmd::load_policy(ctx, &root)?;
        let view = horch_core::routing::snapshot::obtain(
            &root,
            horch_core::clock::now(),
            &policy,
            true,
            &super::quotacmd::quota_env(ctx),
        )?;
        let flags = GateFlags {
            exact: args.exact,
            force: args.force,
        };
        let decision = decision::decide(&plan.teammate, &roster, &view, policy.balance_mode, flags);
        if let Some(routing) = RoutingProvenance::from_decision(
            &plan.teammate,
            &roster,
            &view,
            &decision,
            RoutingMode::for_gate(policy.balance_mode, flags),
        )? {
            plan.routing = routing;
        }
        if let Some(line) = decision.line() {
            crate::output::println(&line);
        }
        match &decision {
            Decision::Refuse { .. } => return Err(Refused.into()),
            Decision::Substitute { via, reason, .. } => {
                let merged = decision::resolve(&plan.teammate, &roster, &decision)
                    .with_context(|| format!("fallback '{via}' vanished from the roster"))?;
                plan.model = merged.model.clone().unwrap_or_default();
                plan.session = fresh_session(merged.agent)?;
                plan.teammate = merged;
                plan.via = Some(via.clone());
                plan.substitution_reason = Some(reason.clone());
                Roster::model_is_spawnable(&plan.model, &plan.teammate.name)?;
            }
            Decision::Spawn { .. } => {}
        }
    }

    if let Some(effort) = &args.effort {
        plan.teammate.effort = Some(effort.clone());
    }
    // Checked against what will launch, so a `--effort` typo or a level this
    // agent/model cannot take fails here rather than in an unwatched pane.
    if let Some(effort) = &plan.teammate.effort {
        if let Some(why) = effort_problem(plan.teammate.agent, Some(&plan.model), effort) {
            bail!("{}: {why}", plan.teammate.name);
        }
    }
    // Reject unusable catalogs before allocating a role or recording a live session.
    validate_selection(&plan.teammate)?;

    // Auto role name: <teammate>-<n> from a per-teammate, per-workspace counter.
    let role = match args.role {
        Some(role) => role,
        None => format!(
            "{}-{}",
            plan.teammate.name,
            mailbox.next_seq(&plan.teammate.name)?
        ),
    };
    if mailbox.role_taken(&role) {
        bail!("role '{role}' already exists in this workspace");
    }

    // Ledger first, brief second, pane last: the worker can assume both exist.
    if plan.session.is_resume() {
        ledger.resume_with_phase(&plan.record_id, &role, &args.task, plan.teammate.phase)?;
    } else {
        ledger.insert(Record {
            record_id: plan.record_id.clone(),
            session_id: Some(session_id_or_empty(&plan.session)),
            agent: plan.teammate.agent.as_str().to_string(),
            tier: plan.teammate.name.clone(),
            model: plan.model.clone(),
            phase: plan.teammate.phase,
            role: role.clone(),
            task: args.task.clone(),
            project: Some(project_dir.clone()),
            workspace_id: Some(mailbox.workspace_id().to_string()),
            via: plan.via.clone(),
            substitution_reason: plan.substitution_reason.clone(),
            routing: Some(plan.routing.clone()),
            ..Record::default()
        })?;
    }
    ledger.set_effort(&plan.record_id, plan.teammate.effort.as_deref())?;
    if plan.session.is_resume() {
        ledger.set_routing(&plan.record_id, Some(&plan.routing))?;
    }

    // A spawned pane is a fresh shell started by the herdr server, so it
    // inherits the user's profile - not the environment `horch spawn` was run
    // with. Without the brief, `HORCH_CLAUDE_BIN=claude horch fleet` would
    // silently have no effect on the workers it spawns, which is exactly when
    // the override is needed most.
    let mut brief = Brief {
        schema: horch_core::messaging::brief::SCHEMA,
        role: role.clone(),
        teammate: plan.teammate.name.clone(),
        agent: plan.teammate.agent.as_str().to_string(),
        model: plan.model.clone(),
        record_id: plan.record_id.clone(),
        session: plan.session.clone(),
        task: args.task.clone(),
        project_dir,
        state_dir: super::path_text(ctx.paths.state_override.as_deref()),
        claude_bin: None,
        codex_bin: None,
        resolved: Some(plan.teammate.clone()),
        teammates_dir: roster_dir,
        workdir: None,
        bin_overrides: Default::default(),
    };
    brief.set_overrides(ctx.bins.overrides.clone());
    mailbox.write_brief(&brief)?;

    let from_pane = match args.from_pane {
        Some(p) => p,
        None => {
            let internal = ctx
                .herdr
                .pane
                .as_ref()
                .context("horch spawn needs --from-pane when not run inside a herdr pane")?;
            herdr.pane_get(internal.as_str())?.pane_id
        }
    };

    let new_pane = herdr.pane_split(&from_pane, args.direction)?;
    let exe = ctx.bins.exe()?;
    let command = PaneShell::host().command_line(&exe, &["worker", role.as_str()]);
    herdr.pane_run(&new_pane, &command)?;

    // Lay the grid out, which is also what puts this pane where it belongs: a
    // split halves its parent, so a fresh worker starts full height beside
    // whichever pane was split and the columns come out 1/2, 1/4, 1/8... The
    // tiler moves it into the next free slot and evens every column. It is
    // deterministic Rust, so the orchestrator spends no tokens on layout and
    // never passes --from-pane or --direction.
    //
    // Best effort: a grid that will not lay out must not fail a spawn whose
    // worker is already running.
    if args.tiling == TilingMode::Disabled {
        if let Ok(layout) = herdr.pane_layout(Some(&new_pane)) {
            crate::cmd::balancecmd::equalize_quietly(ctx, &herdr, layout);
        }
    } else {
        crate::cmd::tilecmd::after_change(ctx, &herdr, mailbox.workspace_id(), Some(&new_pane));
    }

    match &plan.session {
        SessionMode::Resume(id) => eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, RESUMING session {id}",
            plan.teammate.agent, plan.model
        ),
        SessionMode::Fresh(None) => eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, new session",
            plan.teammate.agent, plan.model
        ),
        SessionMode::Fresh(Some(id)) => eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, new session {id}",
            plan.teammate.agent, plan.model
        ),
    }
    Ok(new_pane)
}

/// Validate the resolved selection before allocating persistent spawn state.
fn validate_selection(teammate: &Teammate) -> Result<()> {
    horch_core::skills::ensure_supported(teammate)
}

/// Explicit task selection wins over the recorded phase and roster default.
fn resolve_phase(
    explicit: Option<Phase>,
    recorded: Option<Phase>,
    default: Option<Phase>,
) -> Option<Phase> {
    explicit.or(recorded).or(default)
}

/// A fresh session, with an id minted now when the agent accepts one.
fn fresh_session(agent: horch_core::teammates::Agent) -> Result<SessionMode> {
    Ok(SessionMode::Fresh(if agent.mints_session_id() {
        Some(SessionId::new(horch_core::mint_uuid())?)
    } else {
        None
    }))
}

/// The ledger's `session_id`: empty until known.
fn session_id_or_empty(session: &SessionMode) -> String {
    session.id().map(|id| id.to_string()).unwrap_or_default()
}

/// What to launch, resolved from either the roster or a ledger record.
struct Plan {
    teammate: Teammate,
    model: String,
    record_id: String,
    /// The id is `None` when the agent mints its own after launch.
    session: SessionMode,
    /// The fallback whose launch settings are used, when substituted.
    via: Option<String>,
    substitution_reason: Option<String>,
    /// What routing decided; written next to `via` (ARC-14).
    routing: RoutingProvenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_selection_is_refused_before_spawn_side_effects() {
        let mut t = Roster::builtin().unwrap().require("opus").unwrap().clone();
        t.skills = vec!["missing-bundle".into()];
        assert!(validate_selection(&t)
            .unwrap_err()
            .to_string()
            .contains("missing-bundle"));
        t.skills.clear();
        t.disable_skills = true;
        assert!(validate_selection(&t).is_err());
    }

    #[test]
    fn phase_override_wins_and_resume_keeps_recorded_selection() {
        assert_eq!(
            resolve_phase(
                Some(Phase::Validation),
                Some(Phase::Research),
                Some(Phase::Plan)
            ),
            Some(Phase::Validation)
        );
        assert_eq!(
            resolve_phase(None, Some(Phase::Research), Some(Phase::Plan)),
            Some(Phase::Research)
        );
        assert_eq!(
            resolve_phase(None, None, Some(Phase::Plan)),
            Some(Phase::Plan)
        );
        assert_eq!(resolve_phase(None, None, None), None);
    }

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
