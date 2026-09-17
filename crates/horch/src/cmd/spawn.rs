//! `horch spawn` - the port of `bin/horch-spawn`.
//!
//! Creates a new herdr pane running a worker agent, either as a FRESH session or
//! RESUMING a previous session id from the project ledger. Prints the new pane id
//! on stdout (human detail goes to stderr) so callers can chain splits when
//! building layouts.

use anyhow::{bail, Context, Result};
use horch_core::herdr::{Direction, Herdr};
use horch_core::ledger::{Ledger, STATUS_WORKING};
use horch_core::mailbox::{Brief, Mailbox};
use horch_core::paneshell::PaneShell;
use horch_core::teammates::{Phase, Roster, Teammate};

pub struct SpawnArgs {
    pub teammate: Option<String>,
    pub task: String,
    pub resume: Option<String>,
    pub phase: Option<Phase>,
    pub role: Option<String>,
    pub from_pane: Option<String>,
    pub direction: Direction,
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
pub fn spawn(args: SpawnArgs) -> Result<String> {
    if args.teammate.is_none() && args.resume.is_none() {
        bail!("give a teammate (e.g. `horch spawn sonnet \"task\"`) or --resume <id>");
    }
    let roster = Roster::load_with(env_override("HORCH_TEAMMATES_DIR").as_deref())?;

    let herdr = Herdr::new();
    let mailbox = Mailbox::resolve(&herdr)
        .context("horch spawn needs HORCH_WORKSPACE_ID set, or to run inside a herdr pane")?;
    std::fs::create_dir_all(mailbox.dir())?;

    // Pin the project dir so the ledger this process writes and the ledger the
    // worker later reads resolve to the same file.
    let project_dir = horch_core::ledger::project_dir()?;
    let project_dir = project_dir.to_string_lossy().into_owned();
    std::env::set_var("HORCH_PROJECT_DIR", &project_dir);

    let ledger = Ledger::open()?;
    let plan = match &args.resume {
        // Resuming: teammate, model and session all come from the record.
        Some(key) => {
            let record = ledger.get(key)?;
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
            teammate.phase = resolve_phase(args.phase, record.phase, teammate.phase);
            Roster::is_spawnable(&teammate)?;
            Plan {
                teammate,
                model: record.model.clone(),
                record_id: record.record_id.clone(),
                session_id,
                resume: true,
            }
        }
        None => {
            let name = args.teammate.clone().expect("checked above");
            let mut teammate = roster.require(&name)?.clone();
            teammate.phase = resolve_phase(args.phase, None, teammate.phase);
            Roster::is_spawnable(&teammate)?;
            Plan {
                model: teammate.model.clone().unwrap_or_default(),
                record_id: horch_core::mint_uuid(),
                // Claude, pi and Prime accept a caller-minted session id, so
                // the ledger knows the resume handle before the agent even
                // starts. Codex and OpenCode reveal theirs only after launch;
                // `horch worker` harvests those into the ledger asynchronously.
                session_id: if teammate.agent.mints_session_id() {
                    horch_core::mint_uuid()
                } else {
                    String::new()
                },
                teammate,
                resume: false,
            }
        }
    };

    // The last gate before launch, and the only one that sees what will ACTUALLY
    // start: `is_spawnable` above reads the teammate FILE's model, but a resume
    // takes its model from the ledger record, so a stale or hand-edited record
    // could otherwise start a second top-tier session behind an ordinary tier
    // name. Both branches pass through here.
    Roster::model_is_spawnable(&plan.model, &plan.teammate.name)?;
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
    if plan.resume {
        ledger.resume_with_phase(&plan.record_id, &role, &args.task, plan.teammate.phase)?;
    } else {
        ledger.add_with_phase(
            &plan.record_id,
            plan.teammate.agent.as_str(),
            plan.teammate.name.as_str(),
            &plan.model,
            &role,
            Some(plan.session_id.as_str()),
            &args.task,
            plan.teammate.phase,
        )?;
    }

    mailbox.write_brief(&Brief {
        role: role.clone(),
        teammate: plan.teammate.name.clone(),
        agent: plan.teammate.agent.as_str().to_string(),
        model: plan.model.clone(),
        record_id: plan.record_id.clone(),
        session_id: plan.session_id.clone(),
        resume: plan.resume,
        task: args.task.clone(),
        project_dir,
        state_dir: std::env::var("HORCH_STATE_DIR")
            .ok()
            .filter(|s| !s.is_empty()),
        claude_bin: env_override("HORCH_CLAUDE_BIN"),
        codex_bin: env_override("HORCH_CODEX_BIN"),
        resolved: Some(plan.teammate.clone()),
        teammates_dir: env_override("HORCH_TEAMMATES_DIR"),
    })?;

    let from_pane = match args.from_pane {
        Some(p) => p,
        None => {
            let internal = std::env::var("HERDR_PANE_ID")
                .ok()
                .filter(|s| !s.is_empty())
                .context("horch spawn needs --from-pane when not run inside a herdr pane")?;
            herdr.pane_get(&internal)?.pane_id
        }
    };

    let new_pane = herdr.pane_split(&from_pane, args.direction)?;
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let command = PaneShell::host().command_line(&exe, &["worker", role.as_str()]);
    herdr.pane_run(&new_pane, &command)?;

    // A split halves its parent, so growing the fleet leaves the columns at
    // 1/2, 1/4, 1/8... Even them out now: `horch layout` reads raggedness off the
    // two rows' exact pane edges, so drifted columns make it advise a split that
    // pushes the grid further out of shape. This no-ops on the intermediate state
    // where one row is a column ahead of the other, and evens the grid once the
    // second split completes the column.
    if let Ok(layout) = herdr.pane_layout(Some(&new_pane)) {
        crate::cmd::balancecmd::equalize_quietly(&herdr, layout);
    }

    if plan.resume {
        eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, RESUMING session {}",
            plan.teammate.agent, plan.model, plan.session_id
        );
    } else if plan.session_id.is_empty() {
        eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, new session",
            plan.teammate.agent, plan.model
        );
    } else {
        eprintln!(
            "spawned {role} (pane {new_pane}): {} {}, new session {}",
            plan.teammate.agent, plan.model, plan.session_id
        );
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

/// A non-empty environment override, to carry into the worker's brief.
///
/// A spawned pane is a fresh shell started by the herdr server, so it inherits
/// the user's profile - not the environment `horch spawn` was run with. Without
/// this, `HORCH_CLAUDE_BIN=claude horch fleet` would silently have no effect on
/// the workers it spawns, which is exactly when the override is needed most.
fn env_override(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// What to launch, resolved from either the roster or a ledger record.
struct Plan {
    teammate: Teammate,
    model: String,
    record_id: String,
    /// Empty when the agent mints its own id after launch.
    session_id: String,
    resume: bool,
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
