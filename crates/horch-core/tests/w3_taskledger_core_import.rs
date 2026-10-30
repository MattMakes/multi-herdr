//! The import of the old JSON ledgers, `unfinished` and `reopened_from` (#150).
//! Real `git` runs in temp repositories; nothing reads the host's state.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::git::Git;
use horch_core::taskledger::import::{self, ImportReport};
use horch_core::taskledger::Store;
use serde_json::{json, Value};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Fixture {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
    store: Store,
}

/// A repo with 1 commit on `main`, an empty state root, an empty ledger.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), "x").unwrap();
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let state = root.join("state");
    std::fs::create_dir_all(state.join("worktrees")).unwrap();
    let store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    Fixture {
        _dir: dir,
        repo,
        state,
        store,
    }
}

impl Fixture {
    fn write(&self, rel: &str, v: Value) {
        std::fs::write(self.state.join(rel), v.to_string()).unwrap();
    }

    fn import(&mut self, dry_run: bool) -> ImportReport {
        import::import(&mut self.store, &Git::new("git"), &self.state, dry_run).unwrap()
    }

    fn rows(&self, sql: &str) -> Vec<Vec<String>> {
        let mut stmt = self.store.conn().prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            (0..n)
                .map(|i| {
                    Ok(r.get::<_, Option<String>>(i)
                        .map(|v| v.unwrap_or("~".into()))?)
                })
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    fn worktree_add(&self, name: &str) -> PathBuf {
        let path = self.repo.join(".worktrees").join(name);
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()],
        );
        path
    }
}

fn session(f: &Fixture, id: &str, extra: Value) -> Value {
    let mut r = json!({
        "record_id": id, "project": f.repo.to_str().unwrap(), "workspace_id": "ws1",
        "tier": "opus", "role": format!("role-{id}"), "task": format!("task {id}"),
        "created_at": "2024-01-01T00:00:00Z", "updated_at": "2024-01-02T00:00:00Z",
        "session_id": format!("s-{id}")
    });
    for (k, v) in extra.as_object().unwrap() {
        r[k] = v.clone();
    }
    r
}

fn done_history(text: &str) -> Value {
    json!([{"event": "done", "text": text}])
}

#[test]
fn each_session_status_maps_to_worker_task_and_findings() {
    let mut f = fixture();
    let records = vec![
        session(
            &f,
            "ok",
            json!({"status": "done", "history": done_history("all good")}),
        ),
        session(
            &f,
            "unlanded",
            json!({"status": "done", "history": done_history("UNLANDED: kept")}),
        ),
        session(&f, "lf", json!({"state": {"state": "launch_failed"}})),
        session(
            &f,
            "failed",
            json!({"state": {"state": "failed"}, "status": "done"}),
        ),
        session(&f, "legacy", json!({})),
        session(&f, "orch", json!({"kind": "orchestrator"})),
        json!({"record_id": "noproject", "task": "x"}),
    ];
    f.write("ledger.json", Value::Array(records));
    let report = f.import(false);
    assert_eq!(
        (
            report.raw_added,
            report.raw_skipped,
            report.workers,
            report.tasks,
            report.map_errors
        ),
        (7, 0, 5, 5, 1)
    );
    let statuses = f.rows(
        "SELECT w.legacy_record_id, w.status, t.status, COALESCE(w.end_reason, '~'), COALESCE(t.summary, '~'), t.title \
         FROM workers w JOIN tasks t ON t.task_id = w.task_id ORDER BY w.legacy_record_id",
    );
    let end = "ended without horch done";
    let sum = "imported: ended without horch done";
    let expect = |id: &str, w: &str, t: &str, reason: &str, summary: &str| {
        vec![
            id.to_string(),
            w.into(),
            t.into(),
            reason.into(),
            summary.into(),
            format!("task {id}"),
        ]
    };
    assert_eq!(
        statuses,
        vec![
            expect("failed", "incomplete", "cancelled", end, sum),
            expect("legacy", "incomplete", "cancelled", end, sum),
            expect("lf", "launch_failed", "cancelled", "~", "~"),
            expect("ok", "done", "done", "~", "all good"),
            expect("unlanded", "done", "done", "~", "UNLANDED: kept"),
        ]
    );
    assert_eq!(
        f.rows("SELECT source_key, mapped_to, map_error FROM import_raw WHERE source_key IN ('orch', 'noproject') ORDER BY source_key"),
        vec![
            vec!["noproject".to_string(), "~".into(), "the record names no project".into()],
            vec!["orch".to_string(), "skipped:orchestrator".into(), "~".into()],
        ]
    );
    let findings = f.rows("SELECT kind, detail FROM import_findings ORDER BY finding_id");
    let s = |s: &str| s.to_string();
    assert_eq!(
        findings,
        vec![
            vec![s("done_not_landed"), s("role-unlanded reported done, but its work did not land: UNLANDED: kept")],
            vec![s("ended_without_done"), s("role-failed (opus) ended without horch done. Task: task failed. Session: s-failed.")],
            vec![s("ended_without_done"), s("role-legacy (opus) ended without horch done. Task: task legacy. Session: s-legacy.")],
        ]
    );
    assert_eq!(report.findings.get("ended_without_done"), Some(&2));
    assert_eq!(report.findings.get("done_not_landed"), Some(&1));
}

#[test]
fn a_second_import_skips_what_it_has_and_a_dry_run_keeps_nothing() {
    let mut f = fixture();
    f.write(
        "ledger.json",
        json!([session(
            &f,
            "a",
            json!({"status": "done", "history": done_history("ok")})
        )]),
    );
    let dry = f.import(true);
    assert_eq!((dry.dry_run, dry.raw_added, dry.workers), (true, 1, 1));
    assert!(f.rows("SELECT 1 FROM import_raw").is_empty());
    assert_eq!(f.import(false).raw_added, 1);
    let again = f.import(false);
    assert_eq!(
        (again.raw_added, again.raw_skipped, again.workers),
        (0, 1, 0)
    );
}

#[test]
fn each_claim_state_maps_to_a_worktree_status() {
    let mut f = fixture();
    let landed_here = f.worktree_add("landed-here");
    let open_here = f.worktree_add("open-here");
    let gone = |n: &str| f.repo.join(".worktrees").join(n);
    let claim = |id: &str, path: &Path, state: &str| json!({"id": id, "path": path.to_str().unwrap(), "branch": id, "base": "main", "state": state});
    let claims = vec![
        claim("c1", &gone("landed-gone"), "landed"),
        claim("c2", &landed_here, "landed"),
        claim("c3", &gone("unlanded-gone"), "unlanded"),
        claim("c4", &gone("open-gone"), "open"),
        claim("c5", &open_here, "open"),
        json!({"id": "c6", "branch": "x"}),
        claim("c7", Path::new("/nowhere/at/all"), "open"),
    ];
    f.write("worktrees/claims.json", Value::Array(claims));
    let report = f.import(false);
    assert_eq!((report.worktrees, report.map_errors), (5, 2));
    let rows = f.rows("SELECT path, status FROM worktrees ORDER BY created_at, status, path");
    let by_path = |name: &str| {
        rows.iter()
            .find(|r| r[0].ends_with(name))
            .map(|r| r[1].clone())
            .unwrap()
    };
    assert_eq!(by_path("landed-gone"), "cleaned");
    assert_eq!(by_path("landed-here"), "landed");
    assert_eq!(by_path("unlanded-gone"), "unlanded");
    assert_eq!(by_path("open-gone"), "abandoned");
    assert_eq!(by_path("open-here"), "open");
    assert_eq!(
        f.rows("SELECT source_key, map_error FROM import_raw WHERE map_error IS NOT NULL ORDER BY source_key"),
        vec![
            vec!["c6".to_string(), "the claim has no path".into()],
            vec!["c7".to_string(), "cannot tell which repository the claim belongs to".into()],
        ]
    );
    assert!(f.rows("SELECT 1 FROM import_findings").is_empty());
}

#[test]
fn an_open_claim_with_unlanded_commits_and_dirty_files_raises_findings() {
    let mut f = fixture();
    let wt = f.worktree_add("busy");
    std::fs::write(wt.join("new.txt"), "n").unwrap();
    git(&wt, &["add", "new.txt"]);
    git(&wt, &["commit", "-q", "-m", "ahead"]);
    std::fs::write(wt.join("dirty.txt"), "d").unwrap();
    // The record of the claim reported done: that is a second finding.
    f.write(
        "ledger.json",
        json!([session(
            &f,
            "rec",
            json!({"status": "done", "history": done_history("fine")})
        )]),
    );
    f.write(
        "worktrees/claims.json",
        json!([{"id": "c1", "path": wt.to_str().unwrap(), "branch": "busy", "base": "main", "state": "open", "record_id": "rec"}]),
    );
    f.import(false);
    let kinds = f.rows("SELECT kind, detail FROM import_findings ORDER BY finding_id");
    let facts = format!(
        "{} (branch busy, base main): on disk, 1 commits not on main, 1 uncommitted files, ",
        wt.display()
    );
    assert_eq!(kinds.len(), 3, "{kinds:?}");
    assert_eq!(kinds[0][0], "worktree_unlanded");
    assert_eq!(kinds[1][0], "done_not_landed");
    assert_eq!(kinds[2][0], "dirty_worktree");
    for k in &kinds {
        assert!(k[1].starts_with(&facts), "{}", k[1]);
        assert!(k[1].ends_with(" bytes"), "{}", k[1]);
    }
    // The claim is tied to the worker of its record.
    assert_eq!(
        f.rows("SELECT CASE WHEN w.worker_id = wt.worker_id AND wt.task_id = w.task_id THEN 'tied' ELSE 'loose' END FROM worktrees wt, workers w"),
        vec![vec!["tied".to_string()]]
    );
}

#[test]
fn a_git_worktree_no_claim_names_is_an_orphan_reported_once() {
    let mut f = fixture();
    let known = f.worktree_add("known");
    let orphan = f.worktree_add("orphan");
    f.write(
        "ledger.json",
        json!([session(
            &f,
            "a",
            json!({"status": "done", "history": done_history("ok")})
        )]),
    );
    f.write(
        "worktrees/claims.json",
        json!([{"id": "c1", "path": known.to_str().unwrap(), "branch": "known", "base": "main", "state": "open"}]),
    );
    f.import(false);
    f.import(false);
    let rows = f.rows("SELECT kind, detail, task_id FROM import_findings");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0][0], "worktree_orphan");
    assert!(
        rows[0][1].starts_with(&format!(
            "{} is a git worktree no claim names (",
            orphan.display()
        )),
        "{}",
        rows[0][1]
    );
    assert!(rows[0][1].ends_with(" bytes). It is reported, never removed by the import."));
}

#[test]
fn a_pending_mail_file_without_an_id_is_keyed_by_its_file_name() {
    let mut f = fixture();
    let pending = f.state.join("mail").join("w1").join("pending");
    std::fs::create_dir_all(&pending).unwrap();
    std::fs::write(pending.join("m-17.json"), json!({"text": "hi"}).to_string()).unwrap();
    std::fs::write(
        pending.join("m-18.json"),
        json!({"id": "given", "text": "yo"}).to_string(),
    )
    .unwrap();
    // A JSON file that is not a session ledger is not a source.
    f.write("other.json", json!([{"no": "record_id"}]));
    let report = f.import(false);
    assert_eq!((report.files.len(), report.raw_added), (2, 2));
    assert_eq!(
        f.rows("SELECT source_kind, source_key FROM import_raw ORDER BY source_key"),
        vec![
            vec!["pending_mail".to_string(), "given".into()],
            vec!["pending_mail".to_string(), "m-17".into()],
        ]
    );
}

/// 1 native run, closed, with the rows `sync_native` reads.
fn native_fixture(f: &Fixture) {
    let c = f.store.conn();
    for sql in [
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/proj', 'now')",
        "INSERT INTO runs (run_id, project_id, slug, finish_steps, status, origin, started_at) VALUES ('r1', 'p1', 'r1', '[]', 'done', 'native', 'now')",
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status, origin, created_at, updated_at) VALUES ('t1', 'r1', 1, 'one', 'a.md', 'dev', 'dev', 'cancelled', 'native', 'now', 'now')",
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status, origin, created_at, updated_at) VALUES ('t2', 'r1', 2, 'two', 'b.md', 'dev', 'dev', 'running', 'native', 'now', 'now')",
        "INSERT INTO workers (worker_id, run_id, persona, host, status, origin, task_id) VALUES ('w2', 'r1', 'dev', 'h', 'incomplete', 'native', 't2')",
        "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, origin, created_at) VALUES ('wt1', 't1', '/proj', '/proj/.worktrees/one', 'one', 'main', 'open', 'native', 'now')",
    ] {
        c.execute(sql, []).unwrap();
    }
}

#[test]
fn unfinished_lists_native_leftovers_once_and_get_finding_reads_one() {
    let f = fixture();
    native_fixture(&f);
    let c = f.store.conn();
    let first = import::unfinished(c, None).unwrap();
    let again = import::unfinished(c, None).unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first, again, "the findings are written once");
    let mut by_kind: Vec<_> = first
        .iter()
        .map(|u| (u.kind.as_str(), u.detail.as_str()))
        .collect();
    by_kind.sort();
    assert_eq!(
        by_kind,
        vec![
            (
                "reaped_never_finished",
                "T-2 (running): a worker died on it and the run ended before another finished it"
            ),
            (
                "task_cancelled_open_worktree",
                "T-1 was cancelled with its worktree /proj/.worktrees/one still open"
            ),
        ]
    );
    let one = first
        .iter()
        .find(|u| u.kind == "reaped_never_finished")
        .unwrap();
    assert_eq!(
        (
            one.task_id.as_deref(),
            one.project.as_deref(),
            one.run.as_deref()
        ),
        (Some("t2"), Some("/proj"), Some("r1"))
    );
    assert_eq!(&import::get_finding(c, one.finding_id).unwrap(), one);
    assert!(import::get_finding(c, 9999)
        .unwrap_err()
        .to_string()
        .contains("no finding 9999"));
    assert_eq!(
        import::unfinished(c, Some(Path::new("/other"))).unwrap(),
        vec![]
    );
    import::decide(c, one.finding_id, "dismissed", "me", "n").unwrap();
    assert_eq!(import::unfinished(c, None).unwrap().len(), 1);
    assert_eq!(
        import::decide(c, one.finding_id, "dismissed", "me", "n")
            .unwrap_err()
            .to_string(),
        format!("finding {} is not open", one.finding_id)
    );
}

#[test]
fn reopened_from_shows_the_finding_and_each_earlier_session() {
    let f = fixture();
    native_fixture(&f);
    let c = f.store.conn();
    assert_eq!(import::reopened_from(c, "t-new").unwrap(), None);
    c.execute(
        "INSERT INTO import_findings (kind, task_id, detail, status, decision_note, created_at) \
         VALUES ('ended_without_done', 't2', 'it ended', 'requeued', 't-new', 'now')",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE workers SET transcript_path = '/tr/1.jsonl', session_id = 's1', summary = 'half', attempt = 1 WHERE worker_id = 'w2'",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO workers (worker_id, run_id, persona, host, status, origin, task_id, attempt) VALUES ('w3', 'r1', 'dev', 'h', 'incomplete', 'native', 't2', 2)",
        [],
    )
    .unwrap();
    assert_eq!(
        import::reopened_from(c, "t-new").unwrap().as_deref(),
        Some(
            "it ended\n    Earlier session s1: transcript /tr/1.jsonl. Summary: half\
             \n    Earlier session (unknown): transcript (unknown). Summary: (none)"
        )
    );
}
