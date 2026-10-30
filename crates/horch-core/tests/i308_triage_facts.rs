//! `horch worktree triage`: the facts of each parked worktree, on a real
//! git repository in a temp dir (#308, section 1).

mod i308_support;

use horch_core::taskledger::triage::{self, Measure, Row};
use i308_support::{commit, git, write, World};

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row {name} in {rows:#?}"))
}

fn collect(w: &World) -> Vec<Row> {
    triage::collect(w.store.conn(), &w.git, &w.root, Measure::Full).unwrap()
}

#[test]
fn a_folder_row_reports_ahead_behind_diffstat_and_uncommitted_counts() {
    let w = World::new();
    let base = w.rev("main");
    let feat = w.worktree("feat", "feat", &base);
    commit(&feat, "src/f1.rs", "1\n2\n3\n");
    commit(&feat, "src/f2.rs", "1\n");
    commit(&w.root, "b.txt", "main moved\n");
    // 1 modified, 1 staged, 2 untracked files, and build output.
    write(&feat, "src/f1.rs", "1\n2\nchanged\n");
    write(&feat, "s.txt", "staged\n");
    git(&feat, &["add", "s.txt"]);
    write(&feat, "u.txt", "small\n");
    write(&feat, "notes/big.txt", &"x".repeat(5000));
    write(&feat, "target/out.bin", &"o".repeat(3000));

    let rows = collect(&w);
    assert_eq!(rows.len(), 1, "{rows:#?}");
    let r = row(&rows, "feat");
    assert_eq!(r.id, "W1");
    assert_eq!(r.origin, "folder");
    assert_eq!(r.branch.as_deref(), Some("feat"));
    assert_eq!(r.base, "main");
    assert_eq!((r.ahead, r.behind), (2, 1));
    assert_eq!(
        (r.diff.files, r.diff.insertions, r.diff.deletions),
        (2, 4, 0)
    );
    assert_eq!(r.diff.top_paths, vec!["src/f1.rs", "src/f2.rs"]);
    let u = &r.uncommitted;
    assert_eq!((u.modified, u.staged, u.untracked), (1, 1, 2));
    assert_eq!(u.largest_untracked, vec!["notes/big.txt", "u.txt"]);
    assert!(!r.superseded);
    assert_eq!(r.conflicts, Some(0));
    let disk = r.disk.as_ref().unwrap();
    assert_eq!(disk.ignored_bytes, 3000);
    assert!(disk.bytes > 3000 + 5000, "{disk:?}");
    assert!(r.age.last_commit_at.is_some());
    assert!(r.age.newest_mtime.is_some());
}

#[test]
fn a_branch_whose_commits_were_cherry_picked_into_base_is_superseded() {
    let w = World::new();
    let base = w.rev("main");
    let picked = w.worktree("picked", "picked", &base);
    let c = commit(&picked, "p.txt", "picked\n");
    commit(&w.root, "b.txt", "main moved\n");
    git(&w.root, &["cherry-pick", c.as_str()]);
    let rows = collect(&w);
    let r = row(&rows, "picked");
    assert_eq!(r.ahead, 1);
    assert!(r.superseded, "{r:#?}");
}

#[test]
fn a_branch_that_does_not_merge_clean_counts_its_conflicts() {
    let w = World::new();
    let base = w.rev("main");
    let clash = w.worktree("clash", "clash", &base);
    commit(&clash, "a.txt", "clash\n");
    commit(&w.root, "a.txt", "main\n");
    let rows = collect(&w);
    let r = row(&rows, "clash");
    assert_eq!(r.conflicts, Some(1), "{r:#?}");
    assert!(!r.superseded);
}

#[test]
fn ids_follow_the_name_order_and_the_main_checkout_is_not_a_row() {
    let w = World::new();
    let base = w.rev("main");
    w.worktree("zed", "zed", &base);
    w.worktree("alpha", "alpha", &base);
    let rows = collect(&w);
    let ids: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.id.as_str(), r.name.as_str()))
        .collect();
    assert_eq!(ids, vec![("W1", "alpha"), ("W2", "zed")]);
}

/// A ledger row: `status`, with a worker in `worker_status` when given.
fn ledger_row(w: &World, name: &str, status: &str, worker_status: Option<&str>) {
    let base = w.rev("main");
    let path = w.worktree(name, name, &base);
    let conn = w.store.conn();
    conn.execute_batch(
        "INSERT OR IGNORE INTO projects VALUES ('p', 'root', 'now');
         INSERT OR IGNORE INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r1', 'p', 'r1', 'active', '2026-10-01T00:00:00Z');",
    )
    .unwrap();
    let worker = worker_status.map(|s| {
        conn.execute(
            "INSERT INTO workers (worker_id, run_id, persona, host, status) VALUES (?1, 'r1', 'opus', 'h', ?2)",
            [format!("w-{name}"), s.to_string()],
        )
        .unwrap();
        format!("w-{name}")
    });
    conn.execute(
        "INSERT INTO worktrees (worktree_id, worker_id, repo_root, path, branch, base, status, created_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, 'main', ?6, '2026-10-02T00:00:00Z', 'check red')",
        rusqlite::params![
            format!("wt-{name}"),
            worker,
            w.root.to_string_lossy(),
            path.to_string_lossy(),
            name,
            status
        ],
    )
    .unwrap();
}

#[test]
fn ledger_rows_are_parked_unless_a_live_worker_holds_them() {
    let w = World::new();
    ledger_row(&w, "unlanded", "unlanded", None);
    ledger_row(&w, "busy", "open", Some("working"));
    ledger_row(&w, "left", "open", Some("done"));
    ledger_row(&w, "gone", "cleaned", None);
    let rows = collect(&w);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // `busy` has a live worker; `gone` is cleaned, so its folder is a
    // folder row again.
    assert_eq!(names, vec!["gone", "left", "unlanded"]);
    let u = row(&rows, "unlanded");
    assert_eq!(u.origin, "ledger");
    assert_eq!(u.worktree_id.as_deref(), Some("wt-unlanded"));
    assert_eq!(u.status.as_deref(), Some("unlanded"));
    assert_eq!(u.age.created_at.as_deref(), Some("2026-10-02T00:00:00Z"));
    assert!(u.why_parked.as_deref().unwrap().contains("check red"));
    assert_eq!(row(&rows, "gone").origin, "folder");
}

#[test]
fn json_has_the_same_rows() {
    let w = World::new();
    let base = w.rev("main");
    w.worktree("one", "one", &base);
    let rows = collect(&w);
    let v: serde_json::Value = serde_json::from_str(&triage::to_json(&rows)).unwrap();
    assert_eq!(v["rows"][0]["id"], "W1");
    assert_eq!(v["rows"][0]["name"], "one");
    assert_eq!(v["rows"][0]["ahead"], 0);
}
