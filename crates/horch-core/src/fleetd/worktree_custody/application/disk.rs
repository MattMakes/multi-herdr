//! The use case of the disk budget of the worktrees.
//!
//! `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never
//! lost" ("Disk"), FM-28, OR-1; MAP.md of fleetd-p3, Q8, Q9 and Q20.
//! A tree is spare when the idea of its task is `merged` or `cancelled`
//! (ticket 33 of fleetd-p6): a row has no idea, so its task names it.
//! [`disk_budget`] runs in part 2 of a pass, after `reconcile_worktrees`.
//! It reads the sizes that the `worktree_list` job stored and runs no git.
//!
//! It queues 1 `worktree_remove` job for each tree of the decision (Q9).
//! The job runs outside the pass, and its guard checks the tree again at
//! its removal.
//!
//! The hold: when the total stays above the budget after each tree of the
//! decision, 1 needs-you item is open with the total and the budget. It is
//! resolved when the decision no longer holds. [`decision`] is also the
//! read of the global hold `WorktreeDisk` (`holds()` of fleet runtime).

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State as JobState};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::sqlite as store;
use crate::fleetd::worktree_custody::application::remove::queue_auto_remove;
use crate::fleetd::worktree_custody::domain::disk::{
    self as rules, Decision, Tree, BUDGET_KEY, GB, NEEDS_YOU_KEY, NEEDS_YOU_KIND,
};

/// What 1 call of [`disk_budget`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The sum of the measured sizes of all trees, in bytes.
    pub total: u64,
    /// The budget, in bytes.
    pub budget: u64,
    /// The trees of the decision that got a new `worktree_remove` run,
    /// oldest first.
    pub removals: Vec<String>,
    /// The cause of the hold when the total stays above the budget: new
    /// dispatch holds, and the needs-you item is open.
    pub hold: Option<String>,
}

/// 1 row that is not `removed`, with the state of the idea of its task and
/// whether its automatic removal failed.
#[cfg_attr(test, derive(Debug, PartialEq))]
struct Measured {
    id: String,
    state: String,
    has_task: bool,
    idea_state: Option<String>,
    created_at: String,
    bytes: Option<u64>,
    removal_failed: bool,
}

/// The decision of the budget of `budget_gb` GB for the trees in `conn`.
pub fn decision(conn: &Connection, budget_gb: u64) -> Result<Decision> {
    let rows = measured(conn)?;
    let trees: Vec<Tree<'_>> = rows
        .iter()
        .map(|r| Tree {
            id: &r.id,
            state: &r.state,
            has_task: r.has_task,
            idea_state: r.idea_state.as_deref(),
            created_at: &r.created_at,
            bytes: r.bytes,
            removal_failed: r.removal_failed,
        })
        .collect();
    Ok(rules::decide(&trees, budget_gb))
}

/// The disk budget in `tx`, at `clock` (part 2 of a pass): queue the
/// removal of each tree of the decision, and open or resolve the needs-you
/// item of the hold.
pub fn disk_budget(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Report> {
    let d = decision(tx, ctx.config.worktree_disk_budget_gb)?;
    let removals = queue_removals(ctx, tx, &d, clock)?;
    sync_hold_item(ctx, tx, &d)?;
    Ok(Report {
        total: d.total,
        budget: d.budget,
        removals,
        hold: d.hold.then(|| rules::hold_cause(&d)),
    })
}

/// Queue 1 `worktree_remove` run of each tree of `d`, oldest first. A
/// tree with an active run, or whose same removal failed, is not queued
/// again. Give the trees with a new run.
fn queue_removals(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    d: &Decision,
    clock: FleetSeconds,
) -> Result<Vec<String>> {
    let mut removals = Vec::new();
    for id in &d.remove {
        let Some(row) = store::get(tx, id)? else {
            continue;
        };
        let cause = "the worktrees are above their disk budget";
        if queue_auto_remove(ctx, tx, &row, cause, clock)? {
            removals.push(id.clone());
        }
    }
    Ok(removals)
}

/// Open the needs-you item of the hold of `d` with the total and the
/// budget, or resolve it when `d` does not hold.
fn sync_hold_item(ctx: &JobCtx<'_>, tx: &Connection, d: &Decision) -> Result<()> {
    if !d.hold {
        ctx.needs_you.resolve_item(tx, NEEDS_YOU_KEY)?;
        return Ok(());
    }
    let text = format!(
        "The worktrees use {:.1} GB, above the budget of {} GB ({BUDGET_KEY}). fleetd holds \
         new dispatch: no landed or rescued tree of a merged or cancelled idea is left to \
         remove, and fleetd never removes an active tree for space. Land, drop or clean up \
         trees, or raise {BUDGET_KEY}.",
        d.total as f64 / GB as f64,
        d.budget / GB
    );
    let item = NewItem {
        key: NEEDS_YOU_KEY,
        kind: NEEDS_YOU_KIND,
        entity: Entity::Worktree,
        entity_id: BUDGET_KEY,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

/// Each row that is not `removed`, with the state of the idea of its task,
/// in 1 statement: 2 queries for each tree made 1 pass with 10,000 trees
/// spend seconds (T-292). The removal of a tree failed when its newest
/// `worktree_remove` run for its path, without `keep_branch`, failed
/// ([`removal_failed`](crate::fleetd::worktree_custody::application::remove::removal_failed)).
fn measured(conn: &Connection) -> Result<Vec<Measured>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT w.worktree_id, w.state, w.task_id IS NOT NULL, w.created_at, w.bytes_on_disk,
             i.state,
             (SELECT j.state FROM jobs j
               WHERE j.kind = '{remove}' AND j.subject = w.worktree_id AND j.input_key = w.path
               ORDER BY j.run DESC LIMIT 1) = '{failed}'
           FROM worktrees w
           LEFT JOIN tasks t ON t.task_id = w.task_id
           LEFT JOIN ideas i ON i.id = t.idea_id
          WHERE w.state <> 'removed'
          ORDER BY w.rowid",
        remove = Kind::WorktreeRemove.as_str(),
        failed = JobState::Failed.as_str(),
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Measured {
            id: r.get(0)?,
            state: r.get(1)?,
            has_task: r.get(2)?,
            created_at: r.get(3)?,
            bytes: r
                .get::<_, Option<i64>>(4)?
                .and_then(|b| u64::try_from(b).ok()),
            idea_state: r.get(5)?,
            removal_failed: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::fleetd::ideas::application::rollup::idea_state_of_task;
    use crate::fleetd::worktree_custody::application::remove::removal_failed;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;

    /// The reader of each tree before T-292: 2 queries for each tree. The
    /// set reader must give the same rows.
    fn measured_one_by_one(conn: &Connection) -> Vec<Measured> {
        let mut stmt = conn
            .prepare(
                "SELECT worktree_id, state, task_id, created_at, bytes_on_disk, path
                   FROM worktrees WHERE state <> 'removed' ORDER BY rowid",
            )
            .unwrap();
        let rows: Vec<(String, String, Option<String>, String, Option<i64>, String)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows.into_iter()
            .map(|(id, state, task_id, created_at, bytes, path)| Measured {
                idea_state: task_id
                    .as_deref()
                    .and_then(|t| idea_state_of_task(conn, t).unwrap()),
                removal_failed: removal_failed(conn, &id, &path).unwrap(),
                has_task: task_id.is_some(),
                bytes: bytes.and_then(|b| u64::try_from(b).ok()),
                id,
                state,
                created_at,
            })
            .collect()
    }

    /// A ledger with 1 run, its idea, and `n` tasks, each with 1 tree.
    fn ledger(n: usize) -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now');",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                for i in 0..n {
                    let task = format!("T{i:04}");
                    tx.execute(
                        "INSERT INTO tasks (task_id, run_id, seq, title, task_file,
                             primary_persona, claimable_by, status, created_at, updated_at, idea_id)
                         VALUES (?1, 'r1', ?2, ?1, 'a.md', 'opus', 'opus', 'running', 'now',
                             'now', ?3)",
                        params![task, i as i64 + 1, idea],
                    )?;
                    tx.execute(
                        "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch,
                             base, status, created_at, bytes_on_disk)
                         VALUES (?1, ?2, '/p', ?3, ?1, 'main', 'open', 'now', ?4)",
                        params![
                            format!("WT{i:04}"),
                            task,
                            format!("/p/.worktrees/{i}"),
                            i as i64
                        ],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        (tmp, store)
    }

    /// A `worktree_remove` run of the tree `id` at `path`.
    fn removal(conn: &Connection, id: &str, key: &str, run: i64, state: &str) {
        conn.execute(
            "INSERT INTO jobs (kind, subject, input_key, run, state, time_limit_s, created_at)
             VALUES ('worktree_remove', ?1, ?2, ?3, ?4, 60, 'now')",
            params![id, key, run, state],
        )
        .unwrap();
    }

    /// T-292: the set read gives the rows of the read of each tree, on a
    /// mixed ledger: a removed tree, a tree with no task, a failed removal,
    /// a failed removal with a later passed run, and a failed removal of
    /// another path of the same tree.
    #[test]
    fn measured_gives_the_rows_of_the_read_of_each_tree() {
        let (_tmp, store) = ledger(6);
        let c = store.conn();
        c.execute_batch(
            "UPDATE worktrees SET state = 'removed' WHERE worktree_id = 'WT0000';
             UPDATE worktrees SET task_id = NULL WHERE worktree_id = 'WT0001';
             UPDATE worktrees SET bytes_on_disk = NULL WHERE worktree_id = 'WT0002';",
        )
        .unwrap();
        removal(c, "WT0002", "/p/.worktrees/2", 1, "failed");
        removal(c, "WT0003", "/p/.worktrees/3", 1, "failed");
        removal(c, "WT0003", "/p/.worktrees/3", 2, "passed");
        removal(c, "WT0004", "/p/.worktrees/other", 1, "failed");
        let want = measured_one_by_one(c);
        assert_eq!(want.len(), 5);
        assert_eq!(
            want.iter().filter(|m| m.removal_failed).count(),
            1,
            "the fixture has 1 failed removal"
        );
        assert_eq!(measured(c).unwrap(), want);
    }

    /// T-292, by count: the disk budget sums the bytes of all trees, so it
    /// stays whole in 1 pass. It reads them in the same statements for 3
    /// trees and for 300.
    #[test]
    fn measured_runs_the_same_statements_for_3_and_300_trees() {
        let count = |n: usize| {
            let (_tmp, store) = ledger(n);
            let (rows, statements) = count_statements(store.conn(), || measured(store.conn()));
            assert_eq!(rows.unwrap().len(), n);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}
