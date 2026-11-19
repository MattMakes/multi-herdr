//! The e2e tests of FM-40 through the binary: the 2 classes of rescue refs
//! (`docs/specs/fleetd/design.md`, FM-40 and I10; ticket 56 of
//! `docs/tickets/fleetd-p4/MAP.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, with 1 feature that has a branch row. The tests write the rescue
//! refs with git, as fleetd writes them before a rebase, and run fleetd
//! passes with `horch ledger watch --once`. A pass reads the rescue refs in
//! its `worktree_list` job and decides their class in the next pass.
//!
//! The tests read no real load, limit or clock for a decision:
//! - the harness gives the machine through `HORCH_MACHINE_FILE`;
//! - the budget comes from the test `fleet.toml`;
//! - the fleet clock moves by one period of the list job for each step;
//! - the calendar time of the retention period is the time of the event
//!   that ended the feature, which the test writes into the ledger. The
//!   expiry reads the calendar clock of the process, and `HORCH_NOW` does
//!   not reach it, so each test writes that time with 1 day of margin to
//!   the 30 day limit.

#![cfg(unix)]

use std::process::Output;

use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as integration, NewBranch, NewFeature,
};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use horch_e2e::harness::{wait_for_or, Harness};

/// The feature of each world, its branch and the folder of its rescue refs.
const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/feature-1";
const RESCUE_1: &str = "refs/horch/rescue/branch/fleet-feature-1/1";
const RESCUE_2: &str = "refs/horch/rescue/branch/fleet-feature-1/2";

/// The key of the needs-you item of the unique budget.
const BUDGET_ITEM: &str = "rescue_unique_budget";

/// 1 step of the fleet clock: the `worktree_list_period_s` of the default
/// `fleet.toml`, so each step reads the rescue refs again.
const LIST_PERIOD_S: i64 = 60;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

// ─── the world of 1 test ────────────────────────────────────────────────

/// A fleet with 1 `open` feature on [`FEATURE_BRANCH`], a git project whose
/// `main` has 1 commit, and no fleetd running: the test starts the test
/// fleetd with `serve_fleetd` when its rows and refs are ready. `fleet_toml`
/// is the test `fleet.toml` of the fleet root.
fn world(name: &str, fleet_toml: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), fleet_toml).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    git_ok(&h, &["branch", FEATURE_BRANCH]);
    seed_feature(&h);
    h
}

/// The rows that the push of `horch spawn` writes for a feature: the run,
/// the feature and the row of its branch.
fn seed_feature(h: &Harness) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            let feature = runs::create_feature_run(tx, &h.project, "main")?;
            assert_eq!(feature, FEATURE);
            let cause = |text| Cause {
                actor: Actor::Horch,
                text,
            };
            let new = NewFeature {
                id: FEATURE,
                name: Some(FEATURE),
                branch: Some(FEATURE_BRANCH),
                primary_branch: "main",
            };
            integration::insert_feature(tx, None, &new, cause("the test adds the feature"))?;
            let branch = NewBranch {
                name: FEATURE_BRANCH,
                feature_id: FEATURE,
                task_id: None,
                owner: "fleetd",
                due_at: 0,
            };
            integration::insert_branch(tx, None, &branch, cause("the feature branch"))?;
            Ok(())
        })
        .unwrap();
}

/// Run git for the test in the project, in the sealed environment, and give
/// the output.
fn git(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = h.git_cmd(args);
    cmd.current_dir(&h.project);
    cmd.output().unwrap()
}

fn git_ok(h: &Harness, args: &[&str]) -> String {
    let o = git(h, args);
    assert!(o.status.success(), "git {args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// The commit that `name` names, or `None`.
fn tip(h: &Harness, name: &str) -> Option<String> {
    let o = git(h, &["rev-parse", "--verify", "--quiet", name]);
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Commit `contents` as `file` on a scratch branch, drop the branch, and
/// give the commit: no branch reaches it, so only a rescue ref can keep it.
fn commit_off_branches(h: &Harness, file: &str, contents: &[u8]) -> String {
    git_ok(h, &["checkout", "--quiet", "-b", "scratch"]);
    std::fs::write(h.project.join(file), contents).unwrap();
    git_ok(h, &["add", file]);
    git_ok(
        h,
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            &format!("add {file}"),
        ],
    );
    let commit = git_ok(h, &["rev-parse", "HEAD"]);
    git_ok(h, &["checkout", "--quiet", "main"]);
    git_ok(h, &["branch", "-D", "scratch"]);
    // The scratch file is not in the tree of `main`.
    let _ = std::fs::remove_file(h.project.join(file));
    commit
}

/// Write the rescue ref `name` at `commit`, create-only, as fleetd does.
fn write_rescue(h: &Harness, name: &str, commit: &str) {
    git_ok(h, &["update-ref", name, commit, ""]);
    assert_eq!(tip(h, name).as_deref(), Some(commit));
}

/// Land `file` with `contents` on `main`, as the merge of a feature does:
/// the work of a rescue ref of the same change is then in a landed branch.
fn land_on_main(h: &Harness, file: &str, contents: &[u8]) {
    std::fs::write(h.project.join(file), contents).unwrap();
    git_ok(h, &["add", file]);
    git_ok(
        h,
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            &format!("land {file}"),
        ],
    );
}

/// Bytes that git cannot compress: `len` bytes of a fixed xorshift stream.
fn noise(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

// ─── the ledger ─────────────────────────────────────────────────────────

/// The values of the first column of `sql`, as text.
fn query(h: &Harness, sql: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// The feature moves to `merged`, and the event of that move gets the
/// calendar time `days` days before now (the retention period starts
/// there).
fn end_feature(h: &Harness, days: u32) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            transition(
                tx,
                None,
                Entity::Feature,
                FEATURE,
                "merged",
                Cause {
                    actor: Actor::Horch,
                    text: "the test merges the feature",
                },
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    set_end_time(h, days);
}

/// Put the end of the feature `days` days before now.
fn set_end_time(h: &Harness, days: u32) {
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let moved = conn
        .execute(
            "UPDATE events SET at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)
             WHERE entity = 'feature' AND entity_id = ?2
               AND kind = 'transition' AND to_state = 'merged'",
            [format!("-{days} days"), FEATURE.to_string()],
        )
        .unwrap();
    assert_eq!(moved, 1, "1 event ended the feature");
}

/// The state of the needs-you item `key`, for each row of that key.
fn item_states(h: &Harness, key: &str) -> Vec<String> {
    query(
        h,
        &format!("SELECT state FROM needs_you WHERE key = '{key}'"),
    )
}

/// What a failed check shows: the jobs, the needs-you items and the rescue
/// refs.
fn board(h: &Harness) -> String {
    format!(
        "jobs: {:?}\nneeds-you: {:?}\nrefs:\n{}\nfleetd.stderr:\n{}",
        query(h, "SELECT kind || ' ' || state FROM jobs ORDER BY id"),
        query(
            h,
            "SELECT key || ' ' || state FROM needs_you ORDER BY created_at"
        ),
        git_ok(h, &["for-each-ref", "refs/horch/rescue/"]),
        std::fs::read_to_string(h.root.join("fleetd.stderr")).unwrap_or_default(),
    )
}

// ─── the passes ─────────────────────────────────────────────────────────

/// Move the fleet clock by 1 period of the list job, and wait until the
/// `worktree_list` job of that period passed. A pass of the test fleetd runs
/// each second, and each pass decides on the rescue refs that the last
/// passed list read. So after 2 rounds, a pass has decided on the list of
/// the first.
fn round(h: &Harness) {
    let before = lists_passed(h);
    h.advance_fleet_clock(LIST_PERIOD_S);
    wait_for_or(
        "a worktree_list job to pass",
        || (lists_passed(h) > before).then_some(()),
        || board(h),
    );
}

/// `n` rounds.
fn rounds(h: &Harness, n: usize) {
    for _ in 0..n {
        round(h);
    }
}

/// The number of `worktree_list` jobs that passed.
fn lists_passed(h: &Harness) -> usize {
    query(
        h,
        "SELECT CAST(id AS TEXT) FROM jobs WHERE kind = 'worktree_list' AND state = 'passed'",
    )
    .len()
}

// ─── horch rescue drop ──────────────────────────────────────────────────

/// `horch rescue drop <name>` as the operator, with fleetd running.
fn drop_as_operator(h: &Harness, name: &str) -> Output {
    h.run(&["rescue", "drop", name])
}

// ─── FM-40 ──────────────────────────────────────────────────────────────

/// FM-40, class `unique`: a rescue ref whose work is in no landed branch
/// stays through each pass, also when its feature ended long ago, and only
/// `horch rescue drop` deletes it.
/// - The feature ended 400 days ago, far past the 30 day retention.
/// - 3 rounds of passes read the ref and leave it.
/// - `horch rescue drop` shows the class `unique`, deletes the ref, and
///   records 1 event.
/// - The next round plans no second deletion and writes no ref back.
#[test]
fn fm40_unique_rescue_ref_is_kept_until_rescue_drop() {
    let h = world("p4-fm40-unique", "pass_period_s = 1\n");
    let commit = commit_off_branches(&h, "work.txt", b"work that no branch has\n");
    write_rescue(&h, RESCUE_1, &commit);
    end_feature(&h, 400);

    h.serve_fleetd();
    rounds(&h, 3);
    assert!(lists_passed(&h) >= 3, "{}", board(&h));
    assert_eq!(
        tip(&h, RESCUE_1).as_deref(),
        Some(commit.as_str()),
        "a pass deleted a unique rescue ref\n{}",
        board(&h)
    );
    assert!(item_states(&h, BUDGET_ITEM).is_empty(), "{}", board(&h));

    let dropped = drop_as_operator(&h, RESCUE_1);
    assert!(
        dropped.status.success(),
        "{}\n{}",
        text(&dropped),
        board(&h)
    );
    let said = String::from_utf8_lossy(&dropped.stdout);
    assert!(said.contains("class: unique"), "{said}");
    assert!(said.contains(&commit), "{said}");
    assert!(said.contains("work.txt"), "{said}");
    assert_eq!(tip(&h, RESCUE_1), None, "the ref is deleted\n{said}");
    let events = query(
        &h,
        &format!(
            "SELECT cause FROM events WHERE entity = 'worktree'
             AND entity_id = '{RESCUE_1}' AND kind = 'note'"
        ),
    );
    assert_eq!(events.len(), 1, "{events:?}");

    rounds(&h, 2);
    assert_eq!(tip(&h, RESCUE_1), None, "a pass wrote the ref back");
}

/// FM-40, class `superseded`: a rescue ref whose work landed expires 30
/// days after its feature ended, and not before.
/// - The feature ended 29 days ago: the ref stays through a round.
/// - The feature ended 31 days ago: the next round deletes the ref and
///   nothing else.
/// - The work of the ref is in `main`, so the delete loses nothing.
#[test]
fn fm40_superseded_rescue_ref_expires_30_days_after_its_feature_ended() {
    let h = world("p4-fm40-superseded", "pass_period_s = 1\n");
    let commit = commit_off_branches(&h, "work.txt", b"work that landed\n");
    write_rescue(&h, RESCUE_1, &commit);
    // The same change lands on `main`: by content the ref is superseded.
    land_on_main(&h, "work.txt", b"work that landed\n");
    // A unique ref of the same feature, as the control: it never expires.
    let other = commit_off_branches(&h, "other.txt", b"work that no branch has\n");
    write_rescue(&h, RESCUE_2, &other);
    end_feature(&h, 29);
    h.serve_fleetd();

    rounds(&h, 3);
    assert_eq!(
        tip(&h, RESCUE_1).as_deref(),
        Some(commit.as_str()),
        "the ref expired 1 day early\n{}",
        board(&h)
    );

    set_end_time(&h, 31);
    wait_for_or(
        "the superseded ref to expire 31 days after the end",
        || tip(&h, RESCUE_1).is_none().then_some(()),
        || board(&h),
    );
    rounds(&h, 2);
    assert_eq!(
        tip(&h, RESCUE_2).as_deref(),
        Some(other.as_str()),
        "the expiry deleted a unique ref\n{}",
        board(&h)
    );
    // The work of the deleted ref is still in `main`.
    let landed = git_ok(&h, &["show", "main:work.txt"]);
    assert_eq!(landed, "work that landed");
}

/// FM-40, the budget: over `rescue_unique_budget_mb`, 1 needs-you item
/// opens and fleetd deletes nothing. The feature ended 400 days ago, so
/// only the rule "unique is never deleted" keeps the 2 refs.
/// - 1 MB is the lowest budget. 2 refs of about 1 MB each hold over 1 MB
///   that no branch has.
/// - Many rounds leave 1 open item, and both refs.
/// - A budget of 100 MB closes the item, and nothing is deleted then
///   either.
#[test]
fn fm40_unique_budget_opens_1_item_and_deletes_nothing() {
    let h = world(
        "p4-fm40-budget",
        "pass_period_s = 1\nrescue_unique_budget_mb = 1\n",
    );
    let first = commit_off_branches(&h, "big-1.bin", &noise(1_100_000));
    write_rescue(&h, RESCUE_1, &first);
    let second = commit_off_branches(&h, "big-2.bin", &noise(1_100_000));
    write_rescue(&h, RESCUE_2, &second);
    end_feature(&h, 400);

    h.serve_fleetd();
    rounds(&h, 3);
    assert_eq!(item_states(&h, BUDGET_ITEM), ["open"], "{}", board(&h));
    for (name, commit) in [(RESCUE_1, &first), (RESCUE_2, &second)] {
        assert_eq!(
            tip(&h, name).as_deref(),
            Some(commit.as_str()),
            "a pass deleted {name}\n{}",
            board(&h)
        );
    }
    let text = query(
        &h,
        &format!("SELECT text FROM needs_you WHERE key = '{BUDGET_ITEM}'"),
    );
    assert!(text[0].contains("rescue_unique_budget_mb"), "{text:?}");
    assert!(text[0].contains("horch rescue drop"), "{text:?}");

    // fleetd reads `fleet.toml` at its start.
    h.stop_fleetd();
    write_fleet_toml(&h, "pass_period_s = 1\nrescue_unique_budget_mb = 100\n");
    h.serve_fleetd();
    rounds(&h, 3);
    assert_eq!(item_states(&h, BUDGET_ITEM), ["resolved"], "{}", board(&h));
    assert!(tip(&h, RESCUE_1).is_some() && tip(&h, RESCUE_2).is_some());
}

/// Rewrite the test `fleet.toml` of the fleet root.
fn write_fleet_toml(h: &Harness, contents: &str) {
    std::fs::write(h.fleet_root().join("fleet.toml"), contents).unwrap();
}

/// FM-40 and I10: only you delete a unique rescue ref. A worker that runs
/// `horch rescue drop` is refused on the client side, no request reaches
/// fleetd, the ref stays and no event is written. The same command with no
/// worker id works.
#[test]
fn rescue_drop_by_a_worker_is_refused() {
    let h = world("p4-fm40-worker-drop", "pass_period_s = 1\n");
    let commit = commit_off_branches(&h, "work.txt", b"work that no branch has\n");
    write_rescue(&h, RESCUE_1, &commit);
    let worker = seed_worker(&h);
    h.serve_fleetd();
    let requests_before = query(&h, "SELECT kind FROM requests");

    let refused = h
        .horch(&["rescue", "drop", RESCUE_1])
        .env("HORCH_WORKER_ID", &worker)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        stderr.contains(&format!(
            "only the operator drops a rescue ref; worker {worker} cannot"
        )),
        "{}",
        text(&refused)
    );
    assert_eq!(
        tip(&h, RESCUE_1).as_deref(),
        Some(commit.as_str()),
        "the refused drop deleted the ref"
    );
    assert_eq!(
        query(&h, "SELECT kind FROM requests"),
        requests_before,
        "a refused worker sent a request"
    );
    let events = query(
        &h,
        &format!(
            "SELECT cause FROM events WHERE entity = 'worktree'
             AND entity_id = '{RESCUE_1}' AND kind = 'note'"
        ),
    );
    assert!(events.is_empty(), "{events:?}");

    // The caller is the cause: the operator drops the same ref.
    let dropped = drop_as_operator(&h, RESCUE_1);
    assert!(dropped.status.success(), "{}", text(&dropped));
    assert_eq!(tip(&h, RESCUE_1), None);
}

/// A ledger worker of the feature run: the caller of a refused drop.
fn seed_worker(h: &Harness) -> String {
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::feature_run(conn, FEATURE).unwrap().expect("the run");
    workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id,
            kind: Some(WorkerKind::Worker),
            persona: "opus".into(),
            ..Default::default()
        },
    )
    .unwrap()
}
