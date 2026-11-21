#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::fleet_runtime::adapters::job_result_file;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::fleetd::worktree_custody::application::gate_tree;
use horch_core::taskledger::model::{Actor, VerifyProfile};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util, Store};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";

const FEATURE_BRANCH: &str = "fleet/r1";

const BRANCHES: [(&str, &str); 3] = [
    ("fleet/r1-T1-a", "f1.txt"),
    ("fleet/r1-T2-b", "f2.txt"),
    ("fleet/r1-T3-c", "f3.txt"),
];

const GATE_FAILS_ON_B: &str = "test ! -e f2.txt";

const GATE_WAITS_FOR_HOLD: &str =
    "while [ -e \"$XDG_CACHE_HOME/../hold\" ]; do sleep 0.1; done; true";

const ABORT_AFTER_COMMIT: &str = "abort-after-commit";

const ABORT_EXIT_CODE: i32 = 86;

const OPEN_BUNDLE: &str = "('building', 'gating', 'passed')";

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

fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

struct World {
    fleetds: Fleetds,
    h: Harness,
    started: usize,
    tips: Vec<String>,
}

impl World {
    fn new(name: &str, fast: &str) -> World {
        let mut h = Harness::new(name).with_git();
        link_bwrap(&h);
        let fleet = h.root.join("fleet");
        let data = h.root.join("data");
        for dir in [&fleet, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_DATA_DIR", data.to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        std::fs::write(fleet.join("fleet.toml"), "pass_period_s = 1\n").unwrap();
        let fast = fast.replace('\\', "\\\\").replace('"', "\\\"");
        std::fs::write(
            h.project.join("fleet.toml"),
            format!("remote = \"none\"\n\n[gates]\nfast = \"{fast}\"\nfull = \"true\"\n"),
        )
        .unwrap();
        let mut w = World {
            fleetds: Fleetds::new(&h),
            h,
            started: 0,
            tips: Vec::new(),
        };
        w.git(&["add", "fleet.toml"]);
        w.git(&["commit", "-q", "--no-gpg-sign", "-m", "gates"]);
        w.git(&["branch", FEATURE_BRANCH, "main"]);
        w.tips = w.seed();
        w
    }

    fn seed(&self) -> Vec<String> {
        let store = Store::open(&self.h.ledger_db()).unwrap();
        let conn = store.conn();
        let run = runs::create(
            conn,
            &self.h.project,
            &NewRun {
                slug: "r1".into(),
                base_branch: Some("main".into()),
                run_branch: Some(FEATURE_BRANCH.into()),
                finish_steps: VerifyProfile::Minimal.steps(),
                ..Default::default()
            },
        )
        .unwrap();
        schema::run_idea(conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("the idea of r1");
        let tip = self.tip(FEATURE_BRANCH).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        let seen = BranchFact::SeenTip(&tip);
        rows::set_branch_fact(&tx, None, FEATURE_BRANCH, seen, cause("its tip")).unwrap();
        let mut tips = Vec::new();
        for (n, (branch, file)) in BRANCHES.iter().enumerate() {
            self.git(&["checkout", "-q", "-b", branch, FEATURE_BRANCH]);
            std::fs::write(self.h.project.join(file), format!("{branch}\n")).unwrap();
            self.git(&["add", file]);
            self.git(&["commit", "-q", "--no-gpg-sign", "-m", file]);
            self.git(&["checkout", "-q", "main"]);
            let new = NewTask::work(&format!("t{n}"), &format!("tickets/t{n}.md"), "opus");
            let task = tasks::add(&tx, &run, &new, Actor::Orchestrator)
                .unwrap()
                .task_id;
            tx.execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&task],
            )
            .unwrap();
            let row = NewBranch {
                name: branch,
                feature_id: FEATURE,
                task_id: Some(&task),
                owner: "worker",
                due_at: 0,
            };
            rows::insert_branch(&tx, None, &row, cause("the bind")).unwrap();
            let limit = BranchFact::RepairAttempts(run.max_attempts);
            rows::set_branch_fact(&tx, None, branch, limit, cause("no worker")).unwrap();
            tips.push(self.tip(branch).unwrap());
        }
        tx.commit().unwrap();
        tips
    }

    fn git(&self, args: &[&str]) -> String {
        let o = self.h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
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

    fn contains(&self, rev: &str, commit: &str) -> bool {
        self.h
            .git_cmd(&["merge-base", "--is-ancestor", commit, rev])
            .status()
            .unwrap()
            .success()
    }

    fn someone_commits(&self, file: &str) -> String {
        let old = self.tip(FEATURE_BRANCH).unwrap();
        let index = self.h.root.join("someone.index");
        let git = |args: &[&str], stdin: Option<&str>| {
            let mut c = self.h.git_cmd(args);
            c.env("GIT_INDEX_FILE", &index);
            if stdin.is_some() {
                c.stdin(std::process::Stdio::piped());
            }
            c.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = c.spawn().unwrap();
            if let Some(body) = stdin {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(body.as_bytes())
                    .unwrap();
            }
            let o = child.wait_with_output().unwrap();
            assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&["read-tree", &old], None);
        let blob = git(&["hash-object", "-w", "--stdin"], Some("someone\n"));
        let info = format!("100644,{blob},{file}");
        git(&["update-index", "--add", "--cacheinfo", &info], None);
        let tree = git(&["write-tree"], None);
        let new = git(
            &["commit-tree", &tree, "-p", &old, "-m", "someone commits"],
            None,
        );
        let branch = format!("refs/heads/{FEATURE_BRANCH}");
        git(&["update-ref", &branch, &new, &old], None);
        new
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn landing(&self, branch: &str) -> String {
        self.conn()
            .query_row(
                "SELECT landing_state FROM branches WHERE name = ?1",
                [branch],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn bundle_state(&self, id: i64) -> String {
        self.conn()
            .query_row("SELECT state FROM bundles WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn bundle_in(&self, states: &[&str]) -> Option<(i64, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id, state FROM bundles ORDER BY id")
            .unwrap();
        let found = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .find(|(_, state)| states.contains(&state.as_str()));
        found
    }

    fn branch_moves(&self, branch: &str, from: &str, to: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT count(*) FROM events
                 WHERE entity_id = ?1 AND from_state = ?2 AND to_state = ?3",
                params![branch, from, to],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn start_fleetd(&mut self) -> u32 {
        self.started += 1;
        let name = format!("fleetd-{}", self.started);
        let pid = self.fleetds.spawn(&self.h, &name);
        wait_for_or(
            "the fleetd row to name the new fleetd",
            || self.fleetds.row().filter(|&(p, _)| p == pid).map(|_| ()),
            || self.evidence(),
        );
        pid
    }

    fn kill_fleetd(&mut self) {
        self.fleetds.kill_current();
    }

    fn run_fleetd_to_fault(&mut self, fault: &str) {
        self.started += 1;
        let name = format!("fleetd-{}", self.started);
        self.h.set("HORCH_FAULT", fault);
        let pid = self.fleetds.spawn(&self.h, &name);
        self.h.unset("HORCH_FAULT");
        let status = self.fleetds.wait_exit(pid);
        assert_eq!(
            status.code(),
            Some(ABORT_EXIT_CODE),
            "the fault {fault} ends {name}\n{}",
            self.evidence()
        );
    }

    fn wait_settled(&self, what: &str) {
        while !self.settled() {
            let before = self.progress();
            wait_for_or(
                &format!("the next step of {what}"),
                || (self.settled() || self.progress() != before).then_some(()),
                || self.evidence(),
            );
        }
    }

    fn progress(&self) -> Vec<String> {
        let mut rows = self.rows("SELECT id || ' ' || state FROM bundles ORDER BY id");
        rows.push(self.count("SELECT count(*) FROM jobs").to_string());
        rows
    }

    fn settled(&self) -> bool {
        let open = self.count(&format!(
            "SELECT count(*) FROM bundles WHERE state IN {OPEN_BUNDLE}"
        ));
        let waiting = BRANCHES
            .iter()
            .filter(|(b, _)| matches!(self.landing(b).as_str(), "ready" | "in_bundle"))
            .count();
        let followed = BRANCHES
            .iter()
            .all(|(b, _)| !matches!(self.landing(b).as_str(), "building" | "verifying"));
        open == 0 && waiting == 0 && followed
    }

    fn evidence(&self) -> String {
        let bundles = self.rows(
            "SELECT id || ' ' || state || ' parent=' || COALESCE(parent_id, '-')
                 || ' base=' || substr(base_tip, 1, 8)
                 || ' ref=' || COALESCE(candidate_ref, '-')
                 || ' job=' || COALESCE(job_id, '-')
             FROM bundles ORDER BY id",
        );
        let branches = self
            .rows("SELECT name || ' ' || COALESCE(landing_state, '-') FROM branches ORDER BY name");
        let jobs = self.rows(
            "SELECT id || ' ' || kind || ' ' || subject || ' run=' || run || ' ' || state
                 || ' ' || COALESCE(substr(result_json, 1, 300), '-')
             FROM jobs ORDER BY id",
        );
        let items = self.rows("SELECT key || ' ' || state FROM needs_you ORDER BY key");
        let stderr: Vec<String> = (1..=self.started)
            .map(|n| {
                let name = format!("fleetd-{n}");
                let out = self.fleetds.output(&name, "stderr");
                let tail: Vec<&str> = out.lines().rev().take(30).collect();
                format!(
                    "{name}.stderr (last lines):\n{}",
                    tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                )
            })
            .collect();
        format!(
            "bundles:\n{}\nbranches:\n{}\njobs:\n{}\nneeds_you:\n{}\n{}",
            bundles.join("\n"),
            branches.join("\n"),
            jobs.join("\n"),
            items.join("\n"),
            stderr.join("\n")
        )
    }

    fn hold(&self) -> PathBuf {
        let dir = gate_tree::cache_dir(&self.h.fleet_root(), FEATURE);
        std::fs::create_dir_all(&dir).unwrap();
        let hold = dir.join("hold");
        std::fs::write(&hold, "").unwrap();
        hold
    }

    fn running_gate(&self, id: i64) -> Option<(i64, i64)> {
        self.conn()
            .query_row(
                "SELECT j.id, j.run FROM jobs j JOIN bundles b ON b.job_id = j.id
                 WHERE b.id = ?1 AND j.kind = 'fast_gate' AND j.state = 'running'",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    fn candidate_refs(&self) -> Vec<String> {
        let out = self.git(&["for-each-ref", "--format=%(refname)", "refs/horch/bundle/"]);
        out.lines().map(str::to_string).collect()
    }
}

fn assert_landed_but_b(w: &World, kept: &[&str]) {
    for ((branch, _), tip) in BRANCHES.iter().zip(&w.tips) {
        let want = if *branch == "fleet/r1-T2-b" {
            "gate_failed"
        } else {
            "landed"
        };
        assert_eq!(w.landing(branch), want, "{branch}\n{}", w.evidence());
        if want == "landed" {
            assert!(
                w.contains(FEATURE_BRANCH, tip),
                "the feature branch has the commit of {branch}\n{}",
                w.evidence()
            );
        }
    }
    for commit in kept {
        assert!(
            w.contains(FEATURE_BRANCH, commit),
            "the feature branch keeps {commit}\n{}",
            w.evidence()
        );
    }
}

#[test]
fn fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice() {
    let mut w = World::new("p4-fm36", GATE_FAILS_ON_B);
    let base = w.tip(FEATURE_BRANCH).unwrap();
    w.start_fleetd();
    let mut stopped_at: BTreeSet<(i64, String)> = BTreeSet::new();
    loop {
        let next = wait_for_or(
            "a bundle in a new state, or the end of the bundles",
            || {
                if let Some(found) = w.bundle_in(&["building", "gating"]) {
                    if !stopped_at.contains(&found) {
                        return Some(Some(found));
                    }
                }
                w.settled().then_some(None)
            },
            || w.evidence(),
        );
        let Some(found) = next else { break };
        w.kill_fleetd();
        stopped_at.insert(found);
        w.start_fleetd();
    }

    assert_landed_but_b(&w, &[&base]);
    let bisected = w.count("SELECT count(*) FROM bundles WHERE parent_id IS NOT NULL");
    assert!(
        bisected >= 2,
        "the failed bundle is bisected\n{}",
        w.evidence()
    );
    let in_bisect = stopped_at.iter().any(|(id, _)| {
        w.conn()
            .query_row("SELECT parent_id FROM bundles WHERE id = ?1", [id], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .unwrap()
            .is_some()
    });
    assert!(
        in_bisect,
        "a fleetd stops while a child bundle of the bisect is in flight: {stopped_at:?}\n{}",
        w.evidence()
    );
    let bundles = w.count("SELECT count(*) FROM bundles");
    for (kind, at_least) in [("bundle_build", bundles), ("fast_gate", 1)] {
        let runs = w.rows(&format!(
            "SELECT subject || ' ' || count(*) || ' ' || max(run) FROM jobs
             WHERE kind = '{kind}' GROUP BY subject"
        ));
        for line in &runs {
            assert!(
                line.ends_with(" 1 1"),
                "{kind}: each bundle has 1 run of 1 job: {line}\n{}",
                w.evidence()
            );
        }
        assert!(
            i64::try_from(runs.len()).unwrap() >= at_least,
            "{kind}: {} bundles with a run of {bundles}\n{}",
            runs.len(),
            w.evidence()
        );
    }
    let abandoned = w.count(
        "SELECT count(*) FROM jobs WHERE kind IN ('bundle_build', 'fast_gate')
         AND state = 'abandoned'",
    );
    assert_eq!(abandoned, 0, "no run is abandoned\n{}", w.evidence());
}

#[test]
fn fm37_passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready() {
    let mut w = World::new("p4-fm37", GATE_WAITS_FOR_HOLD);
    let hold = w.hold();
    w.start_fleetd();
    let (id, job) = wait_for_or(
        "the fast gate of the first bundle to run",
        || {
            let (id, _) = w.bundle_in(&["gating"])?;
            let job = w.running_gate(id)?;
            Some((id, job))
        },
        || w.evidence(),
    );
    w.kill_fleetd();
    std::fs::remove_file(&hold).unwrap();
    let result = job_result_file::path(&w.h.fleet_root(), job.0, job.1);
    wait_for_or(
        "the fast gate job to write its result",
        || result.is_file().then_some(()),
        || w.evidence(),
    );
    let base: String = w
        .conn()
        .query_row("SELECT base_tip FROM bundles WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    let until = std::time::Instant::now() + horch_e2e::harness::DEADLINE;
    while w.bundle_state(id) != "passed" {
        assert_eq!(w.bundle_state(id), "gating", "{}", w.evidence());
        assert!(
            std::time::Instant::now() < until,
            "timed out waiting for a pass to move the bundle to passed\n{}",
            w.evidence()
        );
        w.run_fleetd_to_fault(ABORT_AFTER_COMMIT);
    }
    assert_eq!(
        w.tip(FEATURE_BRANCH).as_deref(),
        Some(base.as_str()),
        "the ref move did not run"
    );
    let someone = w.someone_commits("someone.txt");

    w.start_fleetd();
    wait_for_or(
        "the passed bundle to be dropped",
        || (w.bundle_state(id) == "dropped").then_some(()),
        || w.evidence(),
    );
    let seen: String = w
        .conn()
        .query_row(
            "SELECT seen_tip FROM branches WHERE name = ?1",
            [FEATURE_BRANCH],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        w.contains(&seen, &someone),
        "fleetd records the new tip {someone}: seen_tip is {seen}\n{}",
        w.evidence()
    );
    for (branch, _) in BRANCHES {
        assert!(
            w.branch_moves(branch, "in_bundle", "ready") >= 1,
            "{branch} goes back to ready\n{}",
            w.evidence()
        );
    }
    w.wait_settled("the members to land on the new tip");
    for ((branch, _), tip) in BRANCHES.iter().zip(&w.tips) {
        assert_eq!(w.landing(branch), "landed", "{branch}\n{}", w.evidence());
        assert!(
            w.contains(FEATURE_BRANCH, tip),
            "{branch}\n{}",
            w.evidence()
        );
    }
    assert!(
        w.contains(FEATURE_BRANCH, &someone),
        "no commit is lost\n{}",
        w.evidence()
    );
}

#[test]
fn candidate_ref_of_each_ended_bundle_is_deleted_and_verified() {
    let mut w = World::new("p4-candidate-refs", GATE_FAILS_ON_B);
    w.start_fleetd();
    let (first, _) = wait_for_or(
        "the first bundle to be in flight",
        || w.bundle_in(&["building", "gating"]),
        || w.evidence(),
    );
    w.kill_fleetd();
    let state = w.bundle_state(first);
    assert!(
        matches!(state.as_str(), "building" | "gating"),
        "the bundle is in flight when fleetd stops: {state}\n{}",
        w.evidence()
    );
    let someone = w.someone_commits("someone.txt");
    w.start_fleetd();
    w.wait_settled("the bundles to end");
    assert_landed_but_b(&w, &[&someone]);
    assert_eq!(w.bundle_state(first), "dropped", "{}", w.evidence());
    for state in ["dropped", "failed", "landed"] {
        let n = w.count(&format!(
            "SELECT count(*) FROM bundles WHERE state = '{state}'"
        ));
        assert!(n >= 1, "a bundle ends {state}\n{}", w.evidence());
    }
    wait_for_or(
        "fleetd to delete and verify each candidate ref",
        || {
            let kept = w.count("SELECT count(*) FROM bundles WHERE candidate_ref IS NOT NULL");
            (kept == 0 && w.candidate_refs().is_empty()).then_some(())
        },
        || format!("refs: {:?}\n{}", w.candidate_refs(), w.evidence()),
    );
    let ended = w.count(&format!(
        "SELECT count(*) FROM bundles WHERE state NOT IN {OPEN_BUNDLE}"
    ));
    assert_eq!(ended, w.count("SELECT count(*) FROM bundles"));
    let built = w.count("SELECT count(*) FROM bundles WHERE candidate_tip IS NOT NULL");
    assert!(built >= 3, "{}", w.evidence());
}
