#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as integration, BranchFact, FeatureFact, NewBranch, NewFeature,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs;
use horch_core::taskledger::Store;
use horch_e2e::harness::{wait_for_or, Harness};

const FEATURE: &str = "F-0001";
const BRANCH: &str = "fleet/feature-1";
const REMOTE: &str = "origin";

const FLEET_TOML: &str = "pass_period_s = 1\nfetch_period_s = 3600\n";

const PROJECT_MANUAL: &str = "pr = \"manual\"\n[gates]\nfast = \"true\"\nfull = \"true\"\n";

const PROJECT_GH: &str = "pr = \"gh\"\n[gates]\nfast = \"true\"\nfull = \"true\"\n";

const PULL_REQUEST: &str = "https://example.test/o/r/pull/1";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct World {
    h: Harness,
    remote: PathBuf,
    tip: String,
}

struct Spec {
    project: &'static str,
    pushed: bool,
    pull_request: Option<&'static str>,
}

fn git(h: &Harness, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = h.git_cmd(args);
    cmd.current_dir(dir);
    cmd.output().unwrap()
}

fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let o = git(h, dir, args);
    assert!(o.status.success(), "git {args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn remote_tip(h: &Harness, remote: &Path, branch: &str) -> Option<String> {
    let o = git(
        h,
        remote,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    );
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn world(name: &str, spec: &Spec) -> World {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");

    let remote = h.root.join("remote.git");
    git_ok(
        &h,
        &h.root,
        &["init", "--quiet", "--bare", &remote.to_string_lossy()],
    );
    git_ok(
        &h,
        &h.project,
        &["remote", "add", REMOTE, &remote.to_string_lossy()],
    );
    git_ok(&h, &h.project, &["push", "--quiet", REMOTE, "main"]);
    git_ok(&h, &h.project, &["checkout", "--quiet", "-b", BRANCH]);
    std::fs::write(h.project.join("fleet.toml"), spec.project).unwrap();
    std::fs::write(h.project.join("feature.txt"), "feature\n").unwrap();
    git_ok(&h, &h.project, &["add", "fleet.toml", "feature.txt"]);
    git_ok(
        &h,
        &h.project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "feature"],
    );
    git_ok(&h, &h.project, &["checkout", "--quiet", "main"]);
    let tip = git_ok(
        &h,
        &h.project,
        &["rev-parse", &format!("refs/heads/{BRANCH}")],
    );
    if spec.pushed {
        git_ok(
            &h,
            &h.project,
            &["push", "--quiet", REMOTE, &format!("{BRANCH}:{BRANCH}")],
        );
    }
    seed_feature(&h, &tip, spec);
    World { h, remote, tip }
}

fn seed_feature(h: &Harness, tip: &str, spec: &Spec) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            let feature = runs::create_feature_run(tx, &h.project, "main")?;
            assert_eq!(feature, FEATURE);
            runs::set_feature_run_branch(tx, FEATURE, BRANCH)?;
            let cause = |text| Cause {
                actor: Actor::Horch,
                text,
            };
            let new = NewFeature {
                id: FEATURE,
                name: Some("feature-1"),
                branch: Some(BRANCH),
                primary_branch: "main",
            };
            integration::insert_feature(tx, None, &new, cause("the test adds the feature"))?;
            let branch = NewBranch {
                name: BRANCH,
                feature_id: FEATURE,
                task_id: None,
                owner: "fleetd",
                due_at: 0,
            };
            integration::insert_branch(tx, None, &branch, cause("the feature branch"))?;
            integration::set_branch_fact(
                tx,
                None,
                BRANCH,
                BranchFact::SeenTip(tip),
                cause("the tip of the feature branch"),
            )?;
            if spec.pushed {
                integration::set_branch_fact(
                    tx,
                    None,
                    BRANCH,
                    BranchFact::SeenRemoteTip(tip),
                    cause("the remote has the tip"),
                )?;
            }
            if let Some(pr) = spec.pull_request {
                integration::set_feature_fact(
                    tx,
                    None,
                    FEATURE,
                    FeatureFact::Pr(pr),
                    cause("the pull request"),
                )?;
            }
            integration::set_feature_fact(
                tx,
                None,
                FEATURE,
                FeatureFact::PrState("open"),
                cause("the pull request is open"),
            )?;
            tx.execute(
                "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES ('I-1', ?1, 'in_progress', '2026-11-18T12:00:00Z', '2026-11-18T12:00:00Z')",
                [FEATURE],
            )?;
            Ok(())
        })
        .unwrap();
}

impl World {
    fn remote_down(&self, down: bool) {
        let off = self.h.root.join("remote.off");
        let (from, to) = if down {
            (&self.remote, &off)
        } else {
            (&off, &self.remote)
        };
        std::fs::rename(from, to).unwrap();
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| {
            Ok(r.get::<_, rusqlite::types::Value>(0)
                .map(|v| match v {
                    rusqlite::types::Value::Null => "NULL".to_string(),
                    rusqlite::types::Value::Integer(n) => n.to_string(),
                    rusqlite::types::Value::Text(s) => s,
                    other => format!("{other:?}"),
                })
                .unwrap())
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
    }

    fn feature_cell(&self, column: &str) -> String {
        let rows = self.rows(&format!(
            "SELECT {column} FROM features WHERE id = '{FEATURE}'"
        ));
        assert_eq!(rows.len(), 1, "the feature row: {rows:?}");
        rows.into_iter().next().unwrap()
    }

    fn job_states(&self, kind: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT state FROM jobs WHERE kind = '{kind}' ORDER BY id"
        ))
    }

    fn open_items(&self, kind: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT key FROM needs_you WHERE kind = '{kind}' AND state = 'open'"
        ))
    }

    fn diagnostics(&self) -> String {
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        format!(
            "jobs: {:?}\nfeature: {:?}\nneeds_you: {:?}\nfleetd.stderr:\n{}",
            self.rows(
                "SELECT id || ' ' || kind || ' ' || run || ' ' || state || ' ' || coalesce(result_json, '') FROM jobs ORDER BY id"
            ),
            self.rows(&format!(
                "SELECT coalesce(push_state, '') || ' ' || push_failures || ' ' || coalesce(push_not_before, '') || ' ' || pr_state FROM features WHERE id = '{FEATURE}'"
            )),
            self.rows("SELECT key || ' ' || state FROM needs_you"),
            read(self.h.root.join("fleetd.stderr")),
        )
    }

    fn wait<T>(&self, what: &str, ready: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, ready, || self.diagnostics())
    }

    fn wait_push_failures(&self, n: usize) {
        self.wait(&format!("{n} failed pushes in a row"), || {
            (self.feature_cell("push_failures") == n.to_string()).then_some(())
        });
    }

    fn fleet_clock(&self) -> i64 {
        self.rows("SELECT fleet_clock_s FROM fleetd WHERE id = 1")[0]
            .parse()
            .unwrap()
    }
}

fn fake_gh(h: &Harness, state: &str) -> PathBuf {
    let log = h.root.join("gh.log");
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nif [ \"$1\" = pr ] && [ \"$2\" = view ]; then\n  echo '{{\"state\":\"{state}\"}}'\nelse\n  exit 1\nfi\n",
        log.display()
    );
    h.write_bin("gh", script.as_bytes());
    log
}

#[test]
fn fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job() {
    let w = world(
        "p4-fm43",
        &Spec {
            project: PROJECT_MANUAL,
            pushed: false,
            pull_request: None,
        },
    );
    w.remote_down(true);
    w.h.serve_fleetd();
    w.wait_push_failures(1);
    assert_eq!(w.feature_cell("push_state"), "failed");
    let not_before: i64 = w.feature_cell("push_not_before").parse().unwrap();
    assert!(
        not_before > w.fleet_clock(),
        "the backoff ends at {not_before}, the clock is {}",
        w.fleet_clock()
    );
    assert_eq!(w.job_states("push"), ["failed"], "{}", w.diagnostics());
    let first = w.rows("SELECT id FROM jobs WHERE kind = 'push'");
    assert!(w.open_items("push_failed").is_empty());

    w.remote_down(false);
    w.h.advance_fleet_clock(1000);
    w.wait("the new run of the push job to pass", || {
        (w.feature_cell("push_state") == "ok").then_some(())
    });
    let states = w.job_states("push");
    assert_eq!(states, ["failed", "passed"], "{}", w.diagnostics());
    let ids = w.rows("SELECT id FROM jobs WHERE kind = 'push'");
    assert_ne!(ids[1], first[0], "the passed run is the failed run");
    assert_eq!(remote_tip(&w.h, &w.remote, BRANCH), Some(w.tip.clone()));
    assert_eq!(w.feature_cell("push_failures"), "0");
    assert_eq!(
        w.rows(&format!(
            "SELECT seen_remote_tip FROM branches WHERE name = '{BRANCH}'"
        )),
        std::slice::from_ref(&w.tip)
    );
    assert!(w.open_items("push_failed").is_empty());
}

#[test]
fn two_fetch_periods_in_a_row_make_two_fetch_runs() {
    let w = world(
        "p4-two-fetches",
        &Spec {
            project: PROJECT_MANUAL,
            pushed: true,
            pull_request: None,
        },
    );
    w.h.serve_fleetd();
    w.wait("the first fetch run to pass", || {
        (w.job_states("fetch") == ["passed"]).then_some(())
    });
    assert_eq!(
        w.rows(&format!(
            "SELECT tip FROM remote_tips WHERE branch = '{BRANCH}'"
        )),
        std::slice::from_ref(&w.tip)
    );
    assert_eq!(
        w.rows(&format!(
            "SELECT stale_since FROM remote_tips WHERE branch = '{BRANCH}'"
        )),
        ["NULL"]
    );

    w.h.advance_fleet_clock(3600);
    w.wait("the second fetch run to pass", || {
        (w.job_states("fetch") == ["passed", "passed"]).then_some(())
    });
    let ids = w.rows("SELECT id FROM jobs WHERE kind = 'fetch'");
    assert_ne!(ids[0], ids[1], "the 2 runs are 1 row");
}

#[test]
fn three_push_failures_open_1_needs_you_item() {
    let w = world(
        "p4-three-failures",
        &Spec {
            project: PROJECT_MANUAL,
            pushed: false,
            pull_request: None,
        },
    );
    w.remote_down(true);
    w.h.serve_fleetd();
    for failures in 1..=2 {
        w.wait_push_failures(failures);
        assert!(
            w.open_items("push_failed").is_empty(),
            "an item after {failures} failures\n{}",
            w.diagnostics()
        );
        w.h.advance_fleet_clock(1000);
    }
    w.wait_push_failures(3);
    let items = w.open_items("push_failed");
    assert_eq!(
        items,
        [format!("push_failed:{FEATURE}")],
        "{}",
        w.diagnostics()
    );
    let body = w.rows("SELECT text FROM needs_you WHERE kind = 'push_failed'");
    assert!(body[0].contains(FEATURE), "{body:?}");
    assert!(body[0].contains("3 times"), "{body:?}");
    assert_eq!(w.feature_cell("push_state"), "failed");
    assert_eq!(w.job_states("push"), ["failed", "failed", "failed"]);
    assert_eq!(remote_tip(&w.h, &w.h.root.join("remote.off"), BRANCH), None);
}

#[test]
fn closed_unmerged_pull_request_opens_1_needs_you_item() {
    let w = world(
        "p4-closed-pr",
        &Spec {
            project: PROJECT_GH,
            pushed: true,
            pull_request: Some(PULL_REQUEST),
        },
    );
    let log = fake_gh(&w.h, "CLOSED");
    w.h.serve_fleetd();
    let key = format!("pr_closed:{FEATURE}");
    w.wait("the item of the closed pull request", || {
        (w.open_items("pr_closed") == [key.clone()]).then_some(())
    });
    let body = w.rows("SELECT text FROM needs_you WHERE kind = 'pr_closed'");
    assert!(body[0].contains("horch feature abandon"), "{body:?}");
    assert_eq!(w.feature_cell("pr_state"), "closed");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        calls.contains(&format!("pr view {PULL_REQUEST}")),
        "gh calls: {calls:?}"
    );
    assert_eq!(w.open_items("pr_closed").len(), 1);
    assert_eq!(w.job_states("fetch"), ["passed"], "{}", w.diagnostics());
}
