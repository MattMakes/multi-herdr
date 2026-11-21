#![cfg(unix)]

use std::cell::RefCell;
use std::process::Output;

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::pass::{
    ABORT_AFTER_COMMIT, ABORT_BEFORE_COMMIT,
};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::runtime::fault::ABORT_EXIT_CODE;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use horch_e2e::harness::{wait_for_or, Harness, DELIVERY_PROBE_FAULT};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection, OptionalExtension};

const FEATURE_BRANCH: &str = "fleet/r1";

const REMOTE: &str = "origin";

const PRIMARY: &str = "main";

const PERSONA: &str = "dev";

const FLEET_TOML: &str = "pass_period_s = 1\n";

const PROJECT_TOML: &str =
    "remote = \"origin\"\npr = \"manual\"\n\n[gates]\nfast = \"true\"\nfull = \"true\"\n";

const MAX_PASSES: usize = 30;

const FEATURE_RESCUES: &str = "refs/horch/rescue/branch/fleet-r1/";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Orchestrator,
        text,
    }
}

fn alive(pid: i64) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 sends nothing; it only checks that the pid exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

struct World {
    h: Harness,
    run: Run,
    passes: RefCell<Vec<String>>,
}

fn world(name: &str) -> World {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let remote = h.root.join("remote.git");
    std::fs::write(h.project.join("fleet.toml"), PROJECT_TOML).unwrap();
    let bare = remote.to_string_lossy().into_owned();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "-q", "--no-gpg-sign", "-m", "the project file"],
        &["init", "-q", "--bare", &bare],
        &["remote", "add", REMOTE, &bare],
        &["push", "-q", REMOTE, PRIMARY],
        &["branch", FEATURE_BRANCH, PRIMARY],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
    let run = seed_run(&h);
    let w = World {
        h,
        run,
        passes: RefCell::new(Vec::new()),
    };
    w.seen();
    w
}

fn seed_run(h: &Harness) -> Run {
    let mut conn = Connection::open(h.ledger_db()).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let root = h.project.canonicalize().unwrap();
    let new = NewRun {
        slug: "r1".into(),
        base_branch: Some(PRIMARY.into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &root, &new).unwrap();
    schema::run_idea(&conn, &run.run_id, &util::now(), true)
        .unwrap()
        .expect("the idea of r1");
    run
}

impl World {
    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn git(&self, args: &[&str]) -> String {
        let o = self.h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn tip(&self, rev: &str) -> Option<String> {
        let o = self
            .h
            .git_cmd(&["rev-parse", "--verify", "-q", rev])
            .output()
            .unwrap();
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    fn contains(&self, tip: &str, ancestor: &str) -> bool {
        self.h
            .git_cmd(&["merge-base", "--is-ancestor", ancestor, tip])
            .status()
            .unwrap()
            .success()
    }

    fn show(&self, rev: &str, file: &str) -> String {
        self.git(&["show", &format!("{rev}:{file}")])
    }

    fn commit_on(&self, branch: &str, from: &str, file: &str, content: &str) -> String {
        if self.tip(&format!("refs/heads/{branch}")).is_some() {
            self.git(&["checkout", "-q", branch]);
        } else {
            self.git(&["checkout", "-q", "-b", branch, from]);
        }
        std::fs::write(self.h.project.join(file), content).unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-q", "--no-gpg-sign", "-m", file]);
        let tip = self.git(&["rev-parse", "HEAD"]);
        self.git(&["checkout", "-q", PRIMARY]);
        tip
    }

    fn land_on_feature(&self, branch: &str) {
        self.git(&["checkout", "-q", FEATURE_BRANCH]);
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "--no-gpg-sign",
            "-m",
            branch,
            branch,
        ]);
        self.git(&["checkout", "-q", PRIMARY]);
    }

    fn seen(&self) {
        let tip = self.tip(FEATURE_BRANCH).unwrap();
        let conn = self.conn();
        let tx = conn.unchecked_transaction().unwrap();
        let fact = BranchFact::SeenTip(&tip);
        rows::set_branch_fact(&tx, None, FEATURE_BRANCH, fact, cause("its tip")).unwrap();
        tx.commit().unwrap();
    }

    fn feature(&self) -> String {
        self.conn()
            .query_row(
                "SELECT id FROM features WHERE branch = ?1",
                [FEATURE_BRANCH],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn task_branch(&self, name: &str, file: &str, content: &str, landing: &str) -> String {
        self.commit_on(name, FEATURE_BRANCH, file, content);
        let feature = self.feature();
        let conn = self.conn();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), PERSONA);
        let task = tasks::add(&conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        conn.execute(
            "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        let row = NewBranch {
            name,
            feature_id: &feature,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        let tx = conn.unchecked_transaction().unwrap();
        rows::insert_branch(&tx, None, &row, cause("the bind")).unwrap();
        tx.execute(
            "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
            params![name, landing],
        )
        .unwrap();
        tx.commit().unwrap();
        task
    }

    fn value(&self, sql: &str, args: impl rusqlite::Params) -> Option<String> {
        let v: Option<SqlValue> = self
            .conn()
            .query_row(sql, args, |r| r.get(0))
            .optional()
            .unwrap();
        match v? {
            SqlValue::Null => None,
            SqlValue::Integer(n) => Some(n.to_string()),
            SqlValue::Text(t) => Some(t),
            other => panic!("{sql}: {other:?}"),
        }
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn.prepare(sql).unwrap();
        let out = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        out
    }

    fn fetched_primary(&self) -> Option<String> {
        self.value("SELECT tip FROM remote_tips WHERE branch = ?1", [PRIMARY])
    }

    fn feature_rescues(&self) -> Vec<(String, String)> {
        let out = self.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            FEATURE_RESCUES,
        ]);
        out.lines()
            .map(|l| {
                let (name, tip) = l.split_once(' ').unwrap();
                (name.to_string(), tip.to_string())
            })
            .collect()
    }

    fn diagnostics(&self) -> String {
        let jobs = self.rows(
            "SELECT id || ' ' || kind || ' ' || subject || ' ' || state || ' '
                 || COALESCE(result_json, '') FROM jobs ORDER BY id",
        );
        let features = self.rows("SELECT id || ' ' || state FROM features");
        let branches = self.rows(
            "SELECT name || ' ' || COALESCE(landing_state, '-') || ' orphan=' || orphan
                 || ' rescue=' || COALESCE(rescue_ref, '-') FROM branches",
        );
        let bundles = self.rows("SELECT id || ' ' || state || ' ' || base_tip FROM bundles");
        let items = self.rows("SELECT key || ' ' || state FROM needs_you");
        let trees = self.rows("SELECT path || ' ' || status FROM worktrees");
        format!(
            "jobs:\n{}\nfeatures: {features:?}\nbranches: {branches:?}\nbundles: {bundles:?}\n\
             needs_you: {items:?}\nworktrees: {trees:?}\npasses:\n{}",
            jobs.join("\n"),
            self.passes.borrow().join("\n")
        )
    }

    fn pass_with(&self, fault: Option<&str>) -> Output {
        let no_clis = self.h.root.join("no-clis");
        std::fs::create_dir_all(&no_clis).unwrap();
        let mut cmd = self
            .h
            .horch(&["ledger", "watch", "--once", "--force", "--json"]);
        cmd.env("PATH", pass_path(&self.h.root, no_clis));
        if let Some(fault) = fault {
            cmd.env("HORCH_FAULT", format!("{DELIVERY_PROBE_FAULT},{fault}"));
        }
        let o = cmd.output().unwrap();
        self.passes.borrow_mut().push(text(&o));
        wait_for_or(
            "the end of each job process",
            || {
                let pids = self.rows(
                    "SELECT CAST(pid AS TEXT) FROM jobs WHERE state = 'running' AND pid IS NOT NULL",
                );
                pids.iter()
                    .all(|p| !alive(p.parse().unwrap()))
                    .then_some(())
            },
            || self.diagnostics(),
        );
        o
    }

    fn pass(&self) {
        let o = self.pass_with(None);
        assert!(o.status.success(), "{}", self.diagnostics());
    }

    fn crash_at(&self, fault: &str) {
        let o = self.pass_with(Some(fault));
        assert_eq!(
            o.status.code(),
            Some(ABORT_EXIT_CODE),
            "the pass ends at {fault}\n{}",
            self.diagnostics()
        );
    }

    fn pass_until<T>(&self, what: &str, mut f: impl FnMut(&World) -> Option<T>) -> T {
        for _ in 0..MAX_PASSES {
            self.pass();
            if let Some(v) = f(self) {
                return v;
            }
        }
        panic!(
            "not after {MAX_PASSES} passes: {what}\n{}",
            self.diagnostics()
        );
    }
}

#[test]
fn i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed() {
    let w = world("p4-i8");
    let task_branch = "fleet/r1-T1-a";
    w.task_branch(task_branch, "notes.txt", "one\nfeature\n", "landed");
    w.land_on_feature(task_branch);
    w.seen();
    let feature_tip = w.tip(FEATURE_BRANCH).unwrap();

    w.commit_on(PRIMARY, PRIMARY, "notes.txt", "one\nmain\n");
    let merge =
        w.h.git_cmd(&["merge", "--squash", FEATURE_BRANCH])
            .output()
            .unwrap();
    assert!(
        !merge.status.success(),
        "the squash conflicts: {}",
        text(&merge)
    );
    w.git(&["checkout", "--theirs", "notes.txt"]);
    w.git(&["add", "notes.txt"]);
    w.git(&["commit", "-q", "--no-gpg-sign", "-m", "squash fleet/r1"]);
    let squash = w.tip(PRIMARY).unwrap();
    w.git(&["push", "-q", REMOTE, PRIMARY]);
    assert!(!w.contains(&squash, &feature_tip), "a squash, not a merge");
    assert_eq!(
        w.show(&squash, "notes.txt"),
        w.show(&feature_tip, "notes.txt")
    );

    let feature = w.feature();
    w.pass_until("the feature is merged", |w| {
        let state = w.value("SELECT state FROM features WHERE id = ?1", [&feature]);
        (state.as_deref() == Some("merged")).then_some(())
    });
    assert_eq!(w.fetched_primary().as_deref(), Some(squash.as_str()));
    let merged = w.value(
        "SELECT count(*) FROM events WHERE entity_id = ?1 AND to_state = 'merged'",
        [&feature],
    );
    assert_eq!(merged.as_deref(), Some("1"), "1 move to merged");
    assert_eq!(w.tip(FEATURE_BRANCH), Some(feature_tip), "no ref moved");
}

#[test]
fn i9_gate_result_on_a_stale_tip_moves_no_ref() {
    let w = world("p4-i9");
    let task_branch = "fleet/r1-T1-a";
    w.task_branch(task_branch, "a.txt", "a\n", "building");
    let base = w.tip(FEATURE_BRANCH).unwrap();

    let bundle = w.pass_until("the fast gate of the bundle ended", |w| {
        w.value(
            "SELECT subject FROM jobs WHERE kind = 'fast_gate' AND state = 'running'",
            [],
        )
    });
    let (state, candidate_ref) = w
        .conn()
        .query_row(
            "SELECT state, candidate_ref FROM bundles WHERE id = ?1",
            [&bundle],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(state, "gating", "{}", w.diagnostics());
    let candidate = w
        .tip(&candidate_ref)
        .expect("the build wrote the candidate");
    assert!(w.contains(&candidate, &base));

    let yours = w.commit_on(FEATURE_BRANCH, FEATURE_BRANCH, "yours.txt", "yours\n");

    w.pass_until("the bundle is dropped", |w| {
        let state = w.value("SELECT state FROM bundles WHERE id = ?1", [&bundle]);
        (state.as_deref() == Some("dropped")).then_some(())
    });
    assert_eq!(w.tip(FEATURE_BRANCH), Some(yours.clone()), "no ref moved");
    assert!(
        !w.contains(&yours, &candidate),
        "the candidate is not on it"
    );
    let gated = w.value(
        "SELECT state FROM jobs WHERE kind = 'fast_gate' AND subject = ?1",
        [&bundle],
    );
    assert_eq!(gated.as_deref(), Some("passed"), "{}", w.diagnostics());
    let landed = w.value(
        "SELECT landing_state FROM branches WHERE name = ?1",
        [task_branch],
    );
    assert_ne!(
        landed.as_deref(),
        Some("landed"),
        "the stale pass lands nothing"
    );
}

#[test]
fn i10_crash_before_and_after_the_rescue_of_a_forced_update_loses_no_tip() {
    let w = world("p4-i10");
    let old = w.commit_on(FEATURE_BRANCH, FEATURE_BRANCH, "feature.txt", "feature\n");
    w.seen();
    let primary = w.commit_on(PRIMARY, PRIMARY, "p.txt", "p\n");
    w.git(&["push", "-q", REMOTE, PRIMARY]);

    w.pass_until("the gate tree of the rebase", |w| {
        let fetched = w.fetched_primary().as_deref() == Some(primary.as_str());
        let made = w.value(
            "SELECT id FROM jobs WHERE kind = 'worktree_add' AND state = 'running'",
            [],
        );
        made.filter(|_| fetched)
    });
    assert_eq!(w.tip(FEATURE_BRANCH), Some(old.clone()));
    assert!(w.feature_rescues().is_empty());

    w.crash_at(ABORT_AFTER_COMMIT);
    assert!(w.feature_rescues().is_empty(), "no rescue ref yet");
    assert_eq!(w.tip(FEATURE_BRANCH), Some(old.clone()), "no tip moved");

    w.pass_until("the rescue ref of the old tip", |w| {
        (!w.feature_rescues().is_empty()).then_some(())
    });
    let rescues = w.feature_rescues();
    assert_eq!(rescues.len(), 1, "{rescues:?}\n{}", w.diagnostics());
    assert_eq!(rescues[0].1, old, "the rescue ref holds the old tip");
    assert_eq!(w.tip(FEATURE_BRANCH), Some(old.clone()), "no rebase yet");

    w.crash_at(ABORT_BEFORE_COMMIT);
    assert_eq!(w.feature_rescues(), rescues);
    assert_eq!(w.tip(FEATURE_BRANCH), Some(old.clone()), "no tip moved");

    let new = w.pass_until("the rebase of the feature branch", |w| {
        let tip = w.tip(FEATURE_BRANCH)?;
        w.contains(&tip, &primary).then_some(tip)
    });
    assert_ne!(new, old);
    assert_eq!(w.show(&new, "feature.txt"), "feature");
    assert_eq!(w.feature_rescues(), rescues, "1 rescue ref, at the old tip");
    w.pass();
    assert_eq!(
        w.tip(FEATURE_BRANCH),
        Some(new),
        "the branch is rebased 1 time"
    );
    let runs = w.value(
        "SELECT count(*) FROM jobs WHERE kind = 'rebase' AND state = 'passed'",
        [],
    );
    assert_eq!(runs.as_deref(), Some("1"), "{}", w.diagnostics());
}

#[test]
fn i11_orphan_fleet_branch_is_adopted_and_not_deleted() {
    let w = world("p4-i11");
    let orphan = "fleet/r1-T9-ghost";
    let tip = w.commit_on(orphan, FEATURE_BRANCH, "ghost.txt", "ghost\n");

    w.pass_until("the orphan row", |w| {
        w.value(
            "SELECT CAST(orphan AS TEXT) FROM branches WHERE name = ?1",
            [orphan],
        )
    });
    for _ in 0..3 {
        w.pass();
    }

    let row = w.value(
        "SELECT CAST(orphan AS TEXT) || ' ' || owner FROM branches WHERE name = ?1",
        [orphan],
    );
    assert_eq!(row.as_deref(), Some("1 operator"), "{}", w.diagnostics());
    let key = format!("orphan_branch:{orphan}");
    let item = w.value("SELECT state FROM needs_you WHERE key = ?1", [&key]);
    assert_eq!(item.as_deref(), Some("open"), "{}", w.diagnostics());
    let opened = w.value(
        "SELECT count(*) FROM events WHERE entity_id = ?1 AND to_state = 'open'",
        [&key],
    );
    assert_eq!(opened.as_deref(), Some("1"), "1 item");
    assert_eq!(w.tip(orphan), Some(tip), "the orphan is not deleted");
}

#[test]
fn i12_stash_entry_blocks_the_deletion_of_its_branch() {
    let w = world("p4-i12");
    let free = "fleet/r1-T1-free";
    let stashed = "fleet/r1-T2-stashed";
    for (branch, file) in [(free, "free.txt"), (stashed, "stashed.txt")] {
        w.task_branch(branch, file, "work\n", "landed");
        w.land_on_feature(branch);
    }
    w.seen();
    let tip = w.tip(stashed).unwrap();
    w.git(&["checkout", "-q", stashed]);
    std::fs::write(w.h.project.join("wip.txt"), "wip\n").unwrap();
    w.git(&["add", "wip.txt"]);
    w.git(&["stash", "push", "-q", "-m", "wip"]);
    w.git(&["checkout", "-q", PRIMARY]);
    let stash = w.git(&["stash", "list"]);
    assert!(stash.contains(stashed), "{stash}");

    w.pass_until("the deletion of the branch that no stash names", |w| {
        w.tip(&format!("refs/heads/{free}")).is_none().then_some(())
    });
    for _ in 0..3 {
        w.pass();
    }

    assert_eq!(w.tip(stashed), Some(tip), "the stash keeps its branch");
    assert_eq!(w.git(&["stash", "list"]), stash, "the stash entry stays");
    let row = w.value(
        "SELECT landing_state FROM branches WHERE name = ?1",
        [stashed],
    );
    assert_eq!(row.as_deref(), Some("landed"));
}

fn pass_path(root: &std::path::Path, no_clis: std::path::PathBuf) -> std::ffi::OsString {
    #[cfg(target_os = "linux")]
    let dirs = vec![no_clis, root.join("host-bin")];
    #[cfg(not(target_os = "linux"))]
    let dirs = {
        let _ = root;
        vec![no_clis]
    };
    std::env::join_paths(dirs).expect("the harness folders hold no PATH separator")
}
