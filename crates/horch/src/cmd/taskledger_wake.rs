//! Waking the orchestrator from the task ledger (section 9, trace-fixes C3):
//! a wake is 1 outbox row first, then a drain moves each undelivered row
//! into durable mail, as `horch tell orchestrator` sends, tagged `[horch]`.
//! A failed send leaves the row for the next drain. A wake never fails the
//! ledger operation that wrote it.

use horch_core::execution::lifecycle::ReportStatus;
use horch_core::ids::WorkspaceId;
use horch_core::messaging::mail;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{Actor, EventKind};
use horch_core::taskledger::outbox::{self, Row, Scope};
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::util::{self, About};
use horch_core::taskledger::Store;
// The wakes that only the tests of this module write: the CLI writes no
// outbox row (RR-5, T-885), and the pass of fleetd writes each wake.
#[cfg(test)]
use horch_core::taskledger::model::OutboxEvent;
#[cfg(test)]
use horch_core::taskledger::outbox::{Wake, Written};
#[cfg(test)]
use horch_core::taskledger::wakes::{self, settled_wake, startable_wake, tagged};
#[cfg(test)]
use horch_core::taskledger::{runs, tasks};
#[cfg(test)]
use rusqlite::Transaction;

/// `HORCH_NO_WAKE=1` turns wakes off (tests that run no herdr). A wake is
/// still written; its send fails, so the row stays undelivered.
fn disabled() -> bool {
    std::env::var("HORCH_NO_WAKE").is_ok_and(|v| v == "1")
}

/// Wake the run's orchestrator with the free-text line `text`. Every call
/// is a new wake, as before the outbox. The event comes from the text
/// ([`event_of`]); a caller that knows its event uses [`wake_as`].
#[cfg(test)]
pub fn wake(ctx: &RuntimeContext, store: &Store, run: &Run, text: &str) {
    let wake = Wake::new(event_of(text), util::new_id(), tagged(text));
    wake_as(ctx, store, run, |_| Ok(Some(wake)));
}

/// The outbox event of a free-text wake. The column allows only the C3
/// events: a stall or suspect line about a worker is `held`.
#[cfg(test)]
fn event_of(text: &str) -> OutboxEvent {
    if text.contains("was reaped") {
        OutboxEvent::Reaped
    } else if text.contains("Spawn") {
        OutboxEvent::Startable
    } else {
        OutboxEvent::Held
    }
}

/// Write the wake that `decide` returns, right after the ledger commit and
/// before any send, then drain the run's outbox. `decide` reads the board
/// in the same transaction as the write. The result is what was written;
/// an error is printed, never returned.
#[cfg(test)]
pub fn wake_as(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    decide: impl FnOnce(&Transaction) -> anyhow::Result<Option<Wake>>,
) -> Written {
    let written = match outbox::write_now(store, &run.run_id, decide) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("horch ledger: the wake was not written: {e:#}");
            return Written::Nothing;
        }
    };
    let new = match &written {
        Written::New(id) => Some(id.as_str()),
        _ => None,
    };
    drain_run(ctx, store, run, new);
    written
}

/// Drain the outbox rows of `run` into durable mail. `new` is the row that
/// the caller just wrote: only its failed send is printed and recorded.
/// An older row failed before, and fails again in silence.
pub fn drain_run(ctx: &RuntimeContext, store: &Store, run: &Run, new: Option<&str>) {
    let drained = outbox::drain(store, Scope::Run(&run.run_id), |row| {
        send_row(ctx, store, run, row, new == Some(row.id.as_str()))
    });
    if let Err(e) = drained {
        eprintln!("horch ledger: the outbox drain failed: {e:#}");
    }
}

/// Send 1 outbox row to the orchestrator of `run` as durable mail, and
/// record the result. `Ok`: the mail is written, and the courier delivers
/// it if the push did not. `Err`: nothing is queued; the row stays.
fn send_row(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    row: &Row,
    first: bool,
) -> anyhow::Result<()> {
    let line = &row.text;
    // The run's workspace wins (#218): the caller can be a pane of another
    // workspace, or a watchdog started from one. The caller's workspace
    // counts only for a run with no workspace.
    let mut ctx = ctx.clone();
    if let Some(id) = run
        .workspace_id
        .as_deref()
        .and_then(|ws| WorkspaceId::new(ws).ok())
    {
        ctx.herdr.workspace = Some(id);
    }
    let target = match &ctx.herdr.workspace {
        Some(ws) if run.workspace_id.is_some() => format!("the run's workspace {}", ws.as_str()),
        Some(ws) => format!("the caller's workspace {}; the run has none", ws.as_str()),
        None => "no workspace".to_string(),
    };
    let sent = if disabled() {
        Err(anyhow::anyhow!("wakes are off (HORCH_NO_WAKE=1)"))
    } else {
        // The row id keys the mail: a replay after a crash before
        // `mark_delivered` reuses the first mail and writes no 2nd file.
        mail::keyed(&row.id, || {
            super::messaging::tell_as(&ctx, Some("horch".into()), "orchestrator", line)
        })
    };
    let note = match &sent {
        Ok(ReportStatus::Delivered) => format!("Wake delivered to {target}: {line}"),
        Ok(ReportStatus::Queued(reason)) => {
            format!("Wake queued, not yet acknowledged ({reason}), for {target}: {line}")
        }
        Err(e) => {
            format!("Wake not delivered ({e:#}), for {target}; it stays in the outbox: {line}")
        }
    };
    if sent.is_ok() || first {
        if let Err(e) = &sent {
            eprintln!("horch ledger: the orchestrator was not woken: {e:#}");
        }
        let _ = util::event(
            store.conn(),
            &About {
                run: Some(&run.run_id),
                ..Default::default()
            },
            Actor::Horch,
            EventKind::Woke,
            &note,
        );
    }
    sent.map(|_| ())
}

/// The settled wake line of `run`, or `None` while the run is not settled
/// (C6: `status::unsettled`, the rule `run close` uses).
#[cfg(test)]
pub(crate) fn settled_note(store: &Store, run: &Run) -> anyhow::Result<Option<String>> {
    wakes::settled_line(store.conn(), run)
}

/// After a task finishes: wake the orchestrator when tasks can start that
/// no live worker is on its way to claim, or when the run is settled.
#[cfg(test)]
pub fn after_done(ctx: &RuntimeContext, store: &Store, run: &Run) {
    if wake_as(ctx, store, run, |tx| settled_wake(tx, run)) != Written::Nothing {
        return;
    }
    nudge_startable(ctx, store, run);
    nudge_stuck(ctx, store, run);
}

/// Wake the orchestrator once when the run has open tasks, none can start
/// and no worker is live: nothing else would ever wake it (#119).
#[cfg(test)]
pub fn nudge_stuck(ctx: &RuntimeContext, store: &Store, run: &Run) {
    wake_as(ctx, store, run, |tx| wakes::stuck_wake(tx, run));
}

/// Wake the orchestrator when workers should be spawned now: 1 row per
/// board state, so a repeat in the same state writes no new row.
#[cfg(test)]
pub fn nudge_startable(ctx: &RuntimeContext, store: &Store, run: &Run) {
    wake_as(ctx, store, run, |tx| startable_wake(tx, run));
}

#[cfg(test)]
mod trf_15_tests {
    use horch_core::taskledger::status;
    use std::path::Path;

    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::NewTask;

    use super::*;

    /// 1 worktree row: (status, folder on disk, triage decision, settled).
    const CASES: &[(&str, bool, Option<&str>, bool)] = &[
        ("open", true, None, false),
        ("landing", true, None, false),
        ("landed", true, None, false),
        ("cleanup_failed", true, None, false),
        ("unlanded", true, None, false),
        ("unlanded", true, Some("KEEP"), true),
        ("unlanded", false, None, true),
        ("abandoned", true, None, false),
        ("abandoned", false, None, true),
        ("cleaned", false, None, true),
        ("archived", false, None, true),
        ("trashed", false, None, true),
    ];

    #[test]
    fn trf_15_settled_wake_agrees_with_status_and_run_close() {
        for &(status, on_disk, decision, settled) in CASES {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
            let run = store
                .immediate(|tx| {
                    let run = runs::create(
                        tx,
                        Path::new("/repo"),
                        &NewRun {
                            slug: "r1".into(),
                            ..Default::default()
                        },
                    )?;
                    // FR-12: a task needs an idea in flight of the feature of its run.
                    horch_core::taskledger::schema::run_idea(
                        tx,
                        &run.run_id,
                        &horch_core::taskledger::util::now(),
                        true,
                    )?;
                    Ok(run)
                })
                .unwrap();
            let t = NewTask::work("alpha", "tickets/alpha.md", "dev");
            let task = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            let wt = dir.path().join("wt");
            if on_disk {
                std::fs::create_dir(&wt).unwrap();
            }
            store
                .conn()
                .execute(
                    "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                    [&task.task_id],
                )
                .unwrap();
            store
                .conn()
                .execute(
                    "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, decision, origin, created_at)
                     VALUES ('wt1', ?1, '/repo', ?2, 'fleet/a', 'main', ?3, ?4, 'native', '2000-01-01T00:00:00Z')",
                    (&task.task_id, wt.to_string_lossy().to_string(), status, decision),
                )
                .unwrap();
            let case = (status, on_disk, decision);
            let note = settled_note(&store, &run).unwrap();
            assert_eq!(note.is_some(), settled, "{case:?}");
            let board = status::report(store.conn(), &run).unwrap();
            assert_eq!(board.complete, settled, "{case:?}");
            let closed = runs::close_run(
                store.conn(),
                &horch_core::taskledger::git::Git::new("git"),
                &run,
                None,
            );
            assert_eq!(closed.is_ok(), settled, "{case:?}: {closed:?}");
        }
    }
}

#[cfg(test)]
mod trf_21_tests {
    use std::path::Path;

    use horch_core::messaging::mail::Mail;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::NewTask;

    use super::*;

    /// A ledger, a run with no workspace, and a context with no workspace
    /// and no herdr pane: a send to the orchestrator fails.
    fn world() -> (tempfile::TempDir, RuntimeContext, Store, Run) {
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
                let run = runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        (dir, ctx, store, run)
    }

    fn rows(store: &Store, run: &Run) -> Vec<(String, Option<String>)> {
        let mut stmt = store
            .conn()
            .prepare("SELECT text, delivered_at FROM outbox WHERE run_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn woke(store: &Store, run: &Run) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT text FROM events WHERE run_id = ?1 AND kind = 'woke' ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([&run.run_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Give the run the workspace `w1`: a send now reaches durable mail.
    fn give_workspace(store: &Store, run: &Run) -> Run {
        store
            .conn()
            .execute(
                "UPDATE runs SET workspace_id = 'w1' WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        runs::get(store.conn(), &run.run_id).unwrap()
    }

    fn orchestrator_mail(ctx: &RuntimeContext) -> Vec<String> {
        let mail = Mail::for_role(&ctx.paths.state_root, "w1", "orchestrator");
        if !mail.dir().is_dir() {
            return Vec::new();
        }
        mail.pending()
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    #[test]
    fn trf_21_a_failed_first_send_leaves_the_row_and_the_next_drain_delivers_it() {
        let (_dir, ctx, store, run) = world();
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");

        // The first send fails: the row stays, and the failure is recorded.
        let r = rows(&store, &run);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].0, "[horch] NOTE: T-1 needs you.");
        assert_eq!(r[0].1, None, "a failed send leaves the row undelivered");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].starts_with("Wake not delivered ("), "{w:?}");
        assert!(w[0].contains("it stays in the outbox"), "{w:?}");
        assert!(orchestrator_mail(&ctx).is_empty());

        // A drain that fails again records nothing new.
        drain_run(&ctx, &store, &run, None);
        assert_eq!(woke(&store, &run).len(), 1);

        // The next drain after the cause is gone delivers into durable mail.
        let run = give_workspace(&store, &run);
        drain_run(&ctx, &store, &run, None);
        let r = rows(&store, &run);
        assert!(r[0].1.is_some(), "the drain set delivered_at: {r:?}");
        let mail = orchestrator_mail(&ctx);
        assert_eq!(mail.len(), 1, "{mail:?}");
        assert!(mail[0].contains("NOTE: T-1 needs you."), "{mail:?}");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[1].contains("the run's workspace w1"), "{w:?}");

        // A delivered row is not sent again.
        drain_run(&ctx, &store, &run, None);
        assert_eq!(orchestrator_mail(&ctx).len(), 1);
    }

    /// A crash after the send queued the mail and before `mark_delivered`
    /// leaves the row undelivered. The next drain sends it again, and the
    /// mailbox still holds 1 pending file with the first mail id.
    #[test]
    fn trf_21_replayed_row_reuses_its_mail() {
        let (_dir, ctx, store, run) = world();
        let run = give_workspace(&store, &run);
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");
        let mail = Mail::for_role(&ctx.paths.state_root, "w1", "orchestrator");
        let first = mail.pending().unwrap();
        assert_eq!(first.len(), 1, "{first:?}");

        // The crash: the row is undelivered again, and its mail stays.
        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_some()));
        let files = std::fs::read_dir(mail.dir().join("pending"))
            .unwrap()
            .filter(|e| {
                let path = e.as_ref().unwrap().path();
                path.extension().is_some_and(|x| x == "json")
            })
            .count();
        assert_eq!(files, 1, "1 row makes 1 pending file");
        let again = mail.pending().unwrap();
        assert_eq!(again[0].id, first[0].id, "the replay reuses the mail id");

        // A replay after the orchestrator acknowledged the mail queues nothing.
        mail.ack_all().unwrap();
        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        assert!(mail.pending().unwrap().is_empty());
    }

    #[test]
    fn trf_21_a_repeat_of_the_same_settled_state_writes_no_new_row() {
        let (_dir, ctx, mut store, run) = world();
        let settled = |store: &Store| {
            rows(store, &run)
                .into_iter()
                .filter(|(t, _)| t.contains("All tasks are done"))
                .count()
        };
        let done = |store: &mut Store, slug: &str| {
            let t = NewTask::work(slug, &format!("tickets/{slug}.md"), "dev");
            let task = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            store
                .conn()
                .execute(
                    "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                    [&task.task_id],
                )
                .unwrap();
        };
        done(&mut store, "alpha");
        after_done(&ctx, &store, &run);
        after_done(&ctx, &store, &run);
        assert_eq!(settled(&store), 1, "1 row per settle");

        // A new task done is a new settled state: 1 more row.
        done(&mut store, "beta");
        after_done(&ctx, &store, &run);
        after_done(&ctx, &store, &run);
        assert_eq!(settled(&store), 2);
    }

    /// Report 45: a crash after the send published the mail and before
    /// `mark_delivered`. The replay finds the mail of the row published and
    /// pushes nothing: the first send pushed it, and the courier has it.
    #[test]
    fn trf_51_replayed_published_row_pushes_nothing() {
        let (_dir, ctx, store, run) = world();
        let run = give_workspace(&store, &run);
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].starts_with("Wake queued"),
            "the first send pushes, and no pane takes it: {w:?}"
        );

        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        let w = woke(&store, &run);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(
            w[1].starts_with("Wake delivered"),
            "the replay tries no push: {w:?}"
        );
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_some()));
        assert_eq!(orchestrator_mail(&ctx).len(), 1, "1 mail");
    }

    #[test]
    fn trf_21_free_text_wakes_get_an_event_and_are_never_deduplicated() {
        let (_dir, ctx, store, run) = world();
        wake(&ctx, &store, &run, "NOTE: same.");
        wake(&ctx, &store, &run, "NOTE: same.");
        assert_eq!(rows(&store, &run).len(), 2);
        assert_eq!(
            event_of("NOTE: w was reaped. Spawn: x"),
            OutboxEvent::Reaped
        );
        assert_eq!(
            event_of("NOTE: worktree triage queued T-1 (FINISH). Spawn a worker for it."),
            OutboxEvent::Startable
        );
        assert_eq!(
            event_of("NOTE: w on T-1 shows no progress"),
            OutboxEvent::Held
        );
    }
}
