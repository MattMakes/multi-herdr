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

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State as JobState};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::task_execution::adapters::sqlite as task_store;
use crate::fleetd::worktree_custody::adapters::sqlite as store;
use crate::fleetd::worktree_custody::application::remove::queue_auto_remove;
use crate::fleetd::worktree_custody::domain::disk::{
    self as rules, Decision, Tree, BUDGET_KEY, GB, NEEDS_YOU_KEY, NEEDS_YOU_KIND,
};
use crate::taskledger::store::one_snapshot;

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

/// The columns of 1 tree row that is not `removed`, as the disk budget
/// reads them.
struct TreeFacts {
    id: String,
    state: String,
    task_id: Option<String>,
    created_at: String,
    bytes: Option<i64>,
    path: String,
}

fn tree_facts(conn: &Connection) -> Result<Vec<TreeFacts>> {
    let mut stmt = conn.prepare(
        "SELECT worktree_id, state, task_id, created_at, bytes_on_disk, path
           FROM worktrees WHERE state <> 'removed' ORDER BY rowid",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TreeFacts {
            id: r.get(0)?,
            state: r.get(1)?,
            task_id: r.get(2)?,
            created_at: r.get(3)?,
            bytes: r.get(4)?,
            path: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 4 statements for any count of trees, on 1 snapshot: the disk budget
/// sums the bytes of all trees, so this read stays whole. The trees, the
/// ideas of their tasks, the states of those ideas and the last removal
/// of each tree are 1 set read each, of the context that owns the table.
fn measured(conn: &Connection) -> Result<Vec<Measured>> {
    one_snapshot(conn, || {
        let trees = tree_facts(conn)?;
        let ideas =
            task_store::ideas_of_tasks(conn, trees.iter().filter_map(|t| t.task_id.as_deref()))?;
        let idea_states = ideas_store::states_of(conn, ideas.values().map(String::as_str))?;
        let removals = jobs_store::latest_states(
            conn,
            Kind::WorktreeRemove,
            trees.iter().map(|t| t.id.as_str()),
        )?;
        let failed = JobState::Failed.as_str();
        Ok(trees
            .into_iter()
            .map(|t| Measured {
                idea_state: t
                    .task_id
                    .as_ref()
                    .and_then(|task| ideas.get(task))
                    .and_then(|idea| idea_states.get(idea))
                    .cloned(),
                removal_failed: removals
                    .get(&(t.id.clone(), t.path))
                    .is_some_and(|state| state == failed),
                has_task: t.task_id.is_some(),
                bytes: t.bytes.and_then(|b| u64::try_from(b).ok()),
                id: t.id,
                state: t.state,
                created_at: t.created_at,
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::fleetd::ideas::application::rollup::idea_state_of_task;
    use crate::fleetd::worktree_custody::application::remove::removal_failed;
    use crate::taskledger::store::Store;
    use crate::taskledger::testing::count_statements;

    fn measured_one_by_one(conn: &Connection) -> Vec<Measured> {
        tree_facts(conn)
            .unwrap()
            .into_iter()
            .map(|t| Measured {
                idea_state: t
                    .task_id
                    .as_deref()
                    .and_then(|task| idea_state_of_task(conn, task).unwrap()),
                removal_failed: removal_failed(conn, &t.id, &t.path).unwrap(),
                has_task: t.task_id.is_some(),
                bytes: t.bytes.and_then(|b| u64::try_from(b).ok()),
                id: t.id,
                state: t.state,
                created_at: t.created_at,
            })
            .collect()
    }

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

    fn removal(conn: &Connection, id: &str, key: &str, run: i64, state: &str) {
        conn.execute(
            "INSERT INTO jobs (kind, subject, input_key, run, state, time_limit_s, created_at)
             VALUES ('worktree_remove', ?1, ?2, ?3, ?4, 60, 'now')",
            params![id, key, run, state],
        )
        .unwrap();
    }

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
