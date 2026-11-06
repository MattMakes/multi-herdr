//! `horch spawn`'s task-ledger side (#34): the worker row is written before
//! the pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.

use std::path::Path;

use anyhow::{bail, ensure, Result};
use horch_core::execution::store::ExecutionStore;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{RunStatus, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{self, Store};

use super::spawn::SpawnArgs;
use super::taskledger::{project_root, RunArg};

/// The fixed task of a worker spawned with no task text.
pub const CLAIM_TASK: &str = "Run horch ledger claim. Read the ticket file it prints and follow it exactly. If it prints that there is no task for you, send the line it prints, then end your turn.";

/// A spawn's worker row.
pub struct LedgerSpawn {
    store: std::cell::RefCell<Store>,
    pub run: Run,
    pub worker_id: String,
}

/// Write the worker row when this spawn joins a task-ledger run: it names
/// one with `--run` (or `HORCH_RUN_ID`), it replaces a ledger worker, or it
/// has no task text and the project has an active run. `None` otherwise: a
/// spawn with task text runs outside every run, with 1 `NOTE:` (#89).
pub fn prepare(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<Option<LedgerSpawn>> {
    let mut store = taskledger::open(ctx)?;
    let Some(explicit) = ledger_run_wanted(ctx, &store, args)? else {
        return Ok(None);
    };
    let run = super::taskledger::resolve_run(ctx, &store, &RunArg { run: explicit })?;
    ensure_active(&run)?;
    let persona = spawn_persona(ctx, args)?;
    ensure_replaceable(&store, args, &run)?;
    let id = insert_row(ctx, &mut store, args, &run, persona)?;
    Ok(Some(LedgerSpawn {
        store: std::cell::RefCell::new(store),
        run,
        worker_id: id,
    }))
}

/// Whether this spawn joins a task-ledger run. `None`: it does not. `Some`
/// holds the run it names (`--run` or `HORCH_RUN_ID`), or `None` for the
/// project's 1 active run. A spawn with task text names no run and says so.
fn ledger_run_wanted(
    ctx: &RuntimeContext,
    store: &Store,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    let root = project_root(ctx)?;
    let explicit = explicit_run(args);
    let active = active_runs(store, &root)?;
    let joins = explicit.is_some() || args.replaces.is_some() || args.task.trim().is_empty();
    if !joins {
        note_outside_run(&active);
        return Ok(None);
    }
    if explicit.is_none() && active.is_empty() {
        ensure!(
            args.replaces.is_none(),
            "--replaces needs an active task-ledger run in {}",
            root.display()
        );
        return Ok(None);
    }
    Ok(Some(explicit))
}

/// The run `--run` or `HORCH_RUN_ID` names.
fn explicit_run(args: &SpawnArgs) -> Option<String> {
    args.run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()))
}

/// The active runs of the project.
fn active_runs(store: &Store, root: &Path) -> Result<Vec<Run>> {
    let all = runs::of_project(store.conn(), root)?;
    Ok(all
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect())
}

/// Insert the worker row of this spawn and return its id.
fn insert_row(
    ctx: &RuntimeContext,
    store: &mut Store,
    args: &SpawnArgs,
    run: &Run,
    persona: String,
) -> Result<String> {
    store.immediate(|tx| {
        workers::insert(
            tx,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona,
                workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
                previous_worker_id: args.replaces.clone(),
                resumed_from_session: args.resume.clone(),
                ..Default::default()
            },
        )
    })
}

/// Say that a spawn with task text runs outside the active runs.
fn note_outside_run(active: &[Run]) {
    if active.is_empty() {
        return;
    }
    let slugs: Vec<&str> = active.iter().map(|r| r.slug.as_str()).collect();
    eprintln!(
        "horch spawn: NOTE: this spawn has task text, so it runs outside task-ledger run {}. Give --run <slug> to join a run.",
        slugs.join(", ")
    );
}

/// A closed run takes no new worker.
fn ensure_active(run: &Run) -> Result<()> {
    ensure!(
        run.status == RunStatus::Active.as_str(),
        "run {} is {}; a closed run takes no new worker",
        run.slug,
        run.status
    );
    Ok(())
}

/// The persona of the new worker: the teammate, or the tier of the
/// execution `--resume` names.
fn spawn_persona(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<String> {
    match (&args.teammate, &args.resume) {
        (Some(t), _) => Ok(t.clone()),
        (None, Some(key)) => Ok(ExecutionStore::open_in(ctx)?.get(key)?.tier),
        (None, None) => bail!("give a teammate or --resume <id>"),
    }
}

/// Refuse to replace a worker that is still live.
fn ensure_replaceable(store: &Store, args: &SpawnArgs, run: &Run) -> Result<()> {
    let Some(prev) = &args.replaces else {
        return Ok(());
    };
    let p = workers::get(store.conn(), prev)?;
    ensure!(
        !p.status()?.is_live(),
        "worker {} is still {}. Run horch ledger stop {} \"<reason>\" --run {}, then spawn again",
        p.label(),
        p.status,
        p.worker_id,
        run.slug
    );
    Ok(())
}

impl LedgerSpawn {
    /// The pane is open and runs `horch worker <role>`.
    pub fn spawned(
        &self,
        role: &str,
        pane: &str,
        harness: &str,
        model: &str,
        effort: Option<&str>,
    ) {
        let r = self.store.borrow_mut().immediate(|tx| {
            workers::set_pane(tx, &self.worker_id, pane)?;
            workers::set_launch(tx, &self.worker_id, role, harness, model, effort)
        });
        if let Err(e) = r {
            eprintln!("horch spawn: the task ledger did not record pane {pane}: {e:#}");
        }
        eprintln!(
            "horch spawn: task ledger worker {} (run {}).",
            self.worker_id, self.run.slug
        );
    }

    /// The spawn failed before the worker ran.
    pub fn launch_failed(&self, reason: &str) {
        let r = self
            .store
            .borrow_mut()
            .immediate(|tx| workers::launch_failed(tx, &self.worker_id, reason));
        if let Err(e) = r {
            eprintln!("horch spawn: the task ledger did not record the failed start: {e:#}");
        }
    }
}

/// What 1 reviewer spawn did (TRF-24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewSpawn {
    /// The worker runs as `role`. `substituted` is the gate's SUBSTITUTED line.
    Spawned {
        role: String,
        substituted: Option<String>,
    },
    /// The gate refused: its REFUSED line.
    Refused(String),
    /// The spawn failed for another reason.
    Failed(String),
}

/// Spawns 1 claiming worker of a persona in a run.
pub trait SpawnReviewer {
    fn spawn(&self, persona: &str, run: &Run) -> ReviewSpawn;
}

/// The normal spawn path: `horch spawn <persona> --run <slug>` as a child
/// process, so the spawn gate decides and its lines stay off this stdout.
pub struct CliSpawn;

impl SpawnReviewer for CliSpawn {
    fn spawn(&self, persona: &str, run: &Run) -> ReviewSpawn {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => return ReviewSpawn::Failed(format!("finding the horch binary: {e}")),
        };
        let out = std::process::Command::new(exe)
            .args(["spawn", persona, "--run", &run.slug])
            .stdin(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) => spawn_outcome(
                o.status.code(),
                &String::from_utf8_lossy(&o.stdout),
                &String::from_utf8_lossy(&o.stderr),
            ),
            Err(e) => ReviewSpawn::Failed(format!("starting horch spawn: {e}")),
        }
    }
}

/// Read the outcome of 1 `horch spawn` from its exit code and output.
pub fn spawn_outcome(code: Option<i32>, stdout: &str, stderr: &str) -> ReviewSpawn {
    let line_of = |prefix: &str| {
        stdout
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with(prefix))
            .map(str::to_string)
    };
    if code == Some(crate::exit::REFUSED as i32) {
        return ReviewSpawn::Refused(line_of("REFUSED:").unwrap_or_else(|| "REFUSED.".into()));
    }
    if code != Some(0) {
        let why = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("horch spawn failed with no message");
        return ReviewSpawn::Failed(why.to_string());
    }
    let role = stderr
        .lines()
        .find_map(|l| l.trim().strip_prefix("spawned "))
        .and_then(|r| r.split(" (pane").next())
        .unwrap_or("a worker")
        .to_string();
    ReviewSpawn::Spawned {
        role,
        substituted: line_of("SUBSTITUTED:"),
    }
}

/// Spawn 1 claiming worker for each queued review task of `of` through
/// `spawner`, then write 1 NOTE wake that names the reviewers. A refused or
/// failed spawn leaves its task queued and says so in the NOTE. Returns
/// the NOTE text.
pub fn spawn_reviewers(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    of: &str,
    reviews: &[String],
    spawner: &dyn SpawnReviewer,
) -> Result<String> {
    let mut spawned = 0;
    let mut items = Vec::new();
    let mut shorts = Vec::new();
    for id in reviews {
        let t = horch_core::taskledger::tasks::get(store.conn(), id)?;
        let persona = t
            .claimable_by
            .clone()
            .or_else(|| t.primary_persona.clone())
            .unwrap_or_default();
        let short = t.short();
        let item = match spawner.spawn(&persona, run) {
            ReviewSpawn::Spawned { role, substituted } => {
                spawned += 1;
                match substituted {
                    Some(s) => format!("{role} ({persona}) is spawned for {short}. {s}"),
                    None => format!("{role} ({persona}) is spawned for {short}."),
                }
            }
            ReviewSpawn::Refused(line) => format!("{short} stays queued. {line}"),
            ReviewSpawn::Failed(why) => {
                format!("{short} stays queued. The spawn of {persona} failed: {why}")
            }
        };
        items.push(item);
        shorts.push(short);
    }
    let text = format!(
        "NOTE: horch spawned {spawned} of {} reviewers for the code-sweep of {of}. {}",
        reviews.len(),
        items.join(" ")
    );
    let wake = horch_core::taskledger::outbox::Wake::new(
        horch_core::taskledger::model::OutboxEvent::Startable,
        format!("reviewers:{}", shorts.join(",")),
        format!("[horch] {text}"),
    );
    super::taskledger_wake::wake_as(ctx, store, run, |_| Ok(Some(wake)));
    Ok(text)
}

#[cfg(test)]
mod trf_24_tests {
    use std::cell::RefCell;

    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::model::{Actor, TaskKind};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::{self, NewTask};

    use super::*;

    /// A fake spawn: 1 outcome per call, in order; it records each persona.
    struct Fake {
        outcomes: RefCell<Vec<ReviewSpawn>>,
        personas: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(outcomes: Vec<ReviewSpawn>) -> Self {
            Fake {
                outcomes: RefCell::new(outcomes),
                personas: RefCell::new(Vec::new()),
            }
        }
    }

    impl SpawnReviewer for Fake {
        fn spawn(&self, persona: &str, _run: &Run) -> ReviewSpawn {
            self.personas.borrow_mut().push(persona.to_string());
            self.outcomes.borrow_mut().remove(0)
        }
    }

    fn spawned(role: &str, substituted: Option<&str>) -> ReviewSpawn {
        ReviewSpawn::Spawned {
            role: role.into(),
            substituted: substituted.map(str::to_string),
        }
    }

    /// A ledger, a run, 1 work task and 1 review task per persona.
    fn world(personas: &[&str]) -> (tempfile::TempDir, RuntimeContext, Store, Run, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", db.to_str().unwrap()),
        )
        .unwrap();
        let mut store = Store::open(&db).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let work = store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "tickets/alpha.md", "opus"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        let mut reviews = Vec::new();
        for (i, p) in personas.iter().enumerate() {
            let mut t = NewTask::work(&format!("Review T-1: r{i}"), "tickets/r.md", p);
            t.kind = TaskKind::Review;
            t.parent_task_id = Some(work.task_id.clone());
            let r = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            reviews.push(r.task_id);
        }
        (dir, ctx, store, run, reviews)
    }

    fn outbox(store: &Store, run: &Run) -> Vec<(String, String)> {
        let mut stmt = store
            .conn()
            .prepare("SELECT event, text FROM outbox WHERE run_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn short(store: &Store, id: &str) -> String {
        tasks::get(store.conn(), id).unwrap().short()
    }

    #[test]
    fn trf_24_horch_spawns_1_reviewer_per_review_task_and_the_orchestrator_gets_1_note() {
        let (_dir, ctx, store, run, reviews) =
            world(&["codex-reviewer", "codex-reviewer", "opus-reviewer"]);
        let fake = Fake::new(vec![
            spawned("codex-reviewer-1", None),
            spawned(
                "codex-reviewer-2",
                Some("SUBSTITUTED: codex-reviewer runs on opus. Reason: codex 7d 100%."),
            ),
            spawned("opus-reviewer-1", None),
        ]);
        let note = spawn_reviewers(&ctx, &store, &run, "T-1", &reviews, &fake).unwrap();
        assert_eq!(
            *fake.personas.borrow(),
            ["codex-reviewer", "codex-reviewer", "opus-reviewer"]
        );
        let rows = outbox(&store, &run);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "startable");
        assert_eq!(rows[0].1, format!("[horch] {note}"));
        assert!(
            note.starts_with("NOTE: horch spawned 3 of 3 reviewers for the code-sweep of T-1."),
            "{note}"
        );
        for (role, id) in [
            ("codex-reviewer-1", &reviews[0]),
            ("codex-reviewer-2", &reviews[1]),
            ("opus-reviewer-1", &reviews[2]),
        ] {
            let item = format!("{role} (");
            assert!(note.contains(&item), "{note}");
            assert!(
                note.contains(&format!("spawned for {}.", short(&store, id))),
                "{note}"
            );
        }
        assert!(
            note.contains("SUBSTITUTED: codex-reviewer runs on opus."),
            "a substitution proceeds and is noted: {note}"
        );
    }

    #[test]
    fn trf_24_a_refused_spawn_leaves_its_task_queued_and_writes_a_note_row() {
        let (_dir, ctx, store, run, reviews) = world(&["codex-reviewer", "opus-reviewer"]);
        let refused = "REFUSED: codex-reviewer cannot start. codex exhausted (7d 100%). Options: wait, --force, or choose a teammate yourself.";
        let fake = Fake::new(vec![
            ReviewSpawn::Refused(refused.into()),
            spawned("opus-reviewer-1", None),
        ]);
        let note = spawn_reviewers(&ctx, &store, &run, "T-1", &reviews, &fake).unwrap();
        let t = tasks::get(store.conn(), &reviews[0]).unwrap();
        assert_eq!(t.status, "queued");
        assert!(t.current_worker_id.is_none());
        assert!(
            note.starts_with("NOTE: horch spawned 1 of 2 reviewers"),
            "{note}"
        );
        assert!(
            note.contains(&format!("{} stays queued. {refused}", t.short())),
            "{note}"
        );
        let rows = outbox(&store, &run);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].1.contains(refused), "{rows:?}");
    }

    #[test]
    fn trf_24_the_spawn_outcome_comes_from_the_exit_code_and_the_gate_lines() {
        let ok = spawn_outcome(
            Some(0),
            "SUBSTITUTED: opus runs on codex-sol. Reason: claude 7d 100%.\nw1:p4\n",
            "horch spawn: task ledger worker abc (run r1).\nspawned codex-sol-3 (pane w1:p4): codex gpt-5, new session\n",
        );
        assert_eq!(
            ok,
            spawned(
                "codex-sol-3",
                Some("SUBSTITUTED: opus runs on codex-sol. Reason: claude 7d 100%.")
            )
        );
        assert_eq!(
            spawn_outcome(
                Some(0),
                "w1:p4\n",
                "spawned opus-2 (pane w1:p4): claude opus, new session\n"
            ),
            spawned("opus-2", None)
        );
        assert_eq!(
            spawn_outcome(
                Some(3),
                "REFUSED: opus cannot start. x.\n",
                "horch: refused\n"
            ),
            ReviewSpawn::Refused("REFUSED: opus cannot start. x.".into())
        );
        assert_eq!(
            spawn_outcome(Some(1), "", "horch: horch spawn refused: enough workers\n"),
            ReviewSpawn::Failed("horch: horch spawn refused: enough workers".into())
        );
    }
}
