//! Phase 0 red tests of landing and features (`docs/specs/fleetd/design.md`,
//! "Features, landing and integration"). Each test is on the list
//! `expected-failures/p0_landing.txt` with the phase that makes it green.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use horch_e2e::harness::{fixtures, Harness};

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.serve_fleetd();
    h
}

/// A harness in workspace `w1` (root pane `w1:p1`) whose panes run their
/// commands, like `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

/// Poll `f` for at most 30 s, so that a test ends in less than 2 min.
fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

fn commit_file(h: &Harness, dir: &Path, file: &str) {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git_in(h, dir, &["add", file]);
    git_in(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
}

/// Commit all the files of the project on its current branch.
fn git_commit_all(h: &Harness, message: &str) {
    let project = h.project.clone();
    git_in(h, &project, &["add", "-A"]);
    git_in(
        h,
        &project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", message],
    );
}

/// Add a bare remote in the harness folder, so that a land never pushes to
/// a real remote.
fn add_bare_remote(h: &Harness) {
    let remote = h.root.join("remote.git");
    let root = h.root.clone();
    let project = h.project.clone();
    git_in(
        h,
        &root,
        &["init", "--quiet", "--bare", &remote.to_string_lossy()],
    );
    git_in(
        h,
        &project,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git_in(h, &project, &["push", "--quiet", "origin", "main"]);
}

/// Declare the fast gate `fast` and the full gate `true` of the project:
/// plan validation refuses a plan without them. The design does not fix the
/// format of this declaration: Phase 4 fixes it and corrects this helper if
/// necessary.
fn declare_gates(h: &Harness, fast: &str) {
    let fast = fast.replace('\\', "\\\\").replace('"', "\\\"");
    std::fs::write(
        h.project.join("fleet.toml"),
        format!("[gates]\nfast = \"{fast}\"\nfull = \"true\"\n"),
    )
    .unwrap();
}

/// The design: when several task branches of 1 feature are `ready`
/// together, fleetd builds 1 candidate of them, the fast gate runs 1 time on
/// the candidate, and each branch lands. Today each finish lands its own
/// task, and no fast gate runs.
#[test]
fn given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten() {
    horch_e2e::expected::check(
        file!(),
        "given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten",
        || {
            let h = world("p0-bundle");
            // The fast gate of the project appends 1 line to the count file
            // each time it runs. The check of each task counts nothing.
            let count = h.root.join("gate-count");
            declare_gates(&h, &format!("/bin/echo run >> {}", count.display()));
            git_commit_all(&h, "gates");
            add_bare_remote(&h);
            ok(&h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
            let check = "true";
            for name in ["01-a", "02-b", "03-c"] {
                let p = ticket(&h, name, &format!("# {name}\n"));
                ok(
                    &h,
                    &[
                        "ledger",
                        "add",
                        "--file",
                        &p.to_string_lossy(),
                        "--primary",
                        "opus",
                        "--check",
                        check,
                    ],
                );
            }
            let mut workers = Vec::new();
            for (name, file) in [("a", "a.txt"), ("b", "b.txt"), ("c", "c.txt")] {
                let w = horch_e2e::bind::bind(&h, "opus", None).worker_id;
                let wt = ok_as(&h, &w, &["ledger", "worktree", "new", name]);
                let path =
                    PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
                commit_file(&h, &path, file);
                workers.push(w);
            }

            // The 3 finishes start at the same time.
            let finishes: Vec<Child> = workers
                .iter()
                .map(|w| {
                    let mut c = h.horch(&["ledger", "finish", "--json"]);
                    c.env("HORCH_WORKER_ID", w)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());
                    c.spawn().unwrap()
                })
                .collect();
            let outputs: Vec<String> = finishes
                .into_iter()
                .map(|c| text(&c.wait_with_output().unwrap()))
                .collect();
            let outputs = outputs.join("\n---\n");

            let project = h.project.clone();
            let log = git_in(&h, &project, &["log", "--format=%s", "fleet/alpha"]);
            let landed: BTreeSet<&str> = log.lines().collect();
            for file in ["a.txt", "b.txt", "c.txt"] {
                assert!(
                    landed.contains(file),
                    "the run branch has no commit {file}: {log}\nfinish output:\n{outputs}"
                );
            }
            let runs = std::fs::read_to_string(&count).unwrap_or_default();
            assert_eq!(
                runs.lines().count(),
                1,
                "the fast gate runs 1 time for the bundle of 3 ready branches; \
                 the fast gate ran {} times\nfinish output:\n{outputs}",
                runs.lines().count()
            );
        },
    );
}

/// Kills a started `horch fleetd` when the test ends, also on a panic.
struct Fleetd(Child);

impl Drop for Fleetd {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Commit `body` as `file` on `branch` with no checkout, because a fleet
/// worktree can hold the branch. Returns the new tip.
fn commit_on_branch(h: &Harness, branch: &str, file: &str, body: &str) -> String {
    let project = h.project.clone();
    let index = h.root.join(format!("index-{}", branch.replace('/', "-")));
    let blob_file = h.root.join("blob");
    std::fs::write(&blob_file, body).unwrap();
    let git = |args: &[&str]| {
        let mut c = h.git_cmd(args);
        c.current_dir(&project).env("GIT_INDEX_FILE", &index);
        let o = c.output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
        out(&o).trim().to_string()
    };
    let old = git(&["rev-parse", &format!("refs/heads/{branch}")]);
    let blob = git(&["hash-object", "-w", &blob_file.to_string_lossy()]);
    git(&["read-tree", &old]);
    git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{blob},{file}"),
    ]);
    let tree = git(&["write-tree"]);
    let new = git(&["commit-tree", &tree, "-p", &old, "-m", branch]);
    git(&["update-ref", &format!("refs/heads/{branch}"), &new, &old]);
    new
}

/// Rows of a query on the ledger of the harness, 1 line for each row.
fn ledger_rows(h: &Harness, sql: &str) -> Vec<String> {
    assert!(h.has_sqlite3(), "this test reads the ledger with sqlite3");
    let db = h.root.join("ledger.sqlite3");
    let o = std::process::Command::new(h.bin.join("sqlite3"))
        .arg(&db)
        .arg(sql)
        .output()
        .unwrap();
    assert!(o.status.success(), "sqlite3 {sql}: {}", text(&o));
    out(&o).lines().map(str::to_string).collect()
}

/// The design: 2 open features that change the same line get a trial merge
/// (`git merge-tree --write-tree`, no checkout). The conflict is a fact in
/// the ledger, each feature keeps its tip, and the conflict reaches a
/// decision. Today there is no `horch idea`, no feature and no trial merge.
#[test]
fn two_conflicting_features_keep_their_tips_and_reach_a_decision() {
    horch_e2e::expected::check(
        file!(),
        "two_conflicting_features_keep_their_tips_and_reach_a_decision",
        || {
            let h = fleet("p0-features", "stay");
            let project = h.project.clone();
            std::fs::write(project.join("shared.txt"), "line 1\n").unwrap();
            declare_gates(&h, "true");
            let retry_ticket = ticket(&h, "01-retry", "# 01: Retry sync\n\n**Check:** true\n");
            let log_ticket = ticket(&h, "02-log", "# 02: Log retries\n\n**Check:** true\n");
            git_commit_all(&h, "base");

            let fleetd = h
                .horch(&["fleetd"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let _fleetd = Fleetd(fleetd);

            // 2 ideas with no `--to`: each one starts its own feature.
            ok(
                &h,
                &["idea", "add", "--direct", &retry_ticket.to_string_lossy()],
            );
            ok(
                &h,
                &["idea", "add", "--direct", &log_ticket.to_string_lossy()],
            );
            // (feature id, feature branch) of each idea, from the ledger:
            // a task branch is not a feature branch.
            let features: Vec<(String, String)> = wait_for("2 feature branches", || {
                let rows = ledger_rows(
                    &h,
                    "SELECT DISTINCT f.id || ' ' || f.branch FROM features f \
                     JOIN ideas i ON i.feature_id = f.id WHERE f.branch IS NOT NULL",
                );
                let rows: Vec<(String, String)> = rows
                    .iter()
                    .filter_map(|r| r.split_once(' '))
                    .map(|(id, b)| (id.to_string(), b.to_string()))
                    .collect();
                (rows.len() >= 2).then_some(rows)
            });
            assert_eq!(features.len(), 2, "2 ideas make 2 features: {features:?}");

            // Each feature branch changes the same line of shared.txt.
            let tips: Vec<String> = features
                .iter()
                .map(|(_, b)| commit_on_branch(&h, b, "shared.txt", &format!("{b}\n")))
                .collect();

            let conflict = wait_for("a trial merge that conflicts", || {
                let rows = ledger_rows(
                    &h,
                    "SELECT left_ref || ' ' || right_ref || ' ' || conflict_files \
                     FROM trial_merges WHERE clean = 0",
                );
                rows.into_iter().find(|r| r.contains("shared.txt"))
            });
            for (_, b) in &features {
                assert!(conflict.contains(b.as_str()), "{conflict} names {b}");
            }

            for ((_, b), tip) in features.iter().zip(&tips) {
                let now = git_in(&h, &project, &["rev-parse", &format!("refs/heads/{b}")]);
                assert_eq!(&now, tip, "feature {b} keeps its tip");
            }

            // The decision: the younger feature is stacked on the older one
            // (and gets 1 resolver task), or 1 needs-you item names both
            // features.
            let ids: BTreeSet<&str> = features.iter().map(|(id, _)| id.as_str()).collect();
            let decision = wait_for("a decision on the conflict", || {
                let stacks = ledger_rows(
                    &h,
                    "SELECT id || ' ' || stacked_on FROM features WHERE stacked_on IS NOT NULL",
                );
                let stacked: Vec<String> = stacks
                    .into_iter()
                    .filter(|r| r.split(' ').collect::<BTreeSet<&str>>() == ids)
                    .collect();
                let items: Vec<String> =
                    ledger_rows(&h, "SELECT text FROM needs_you WHERE state = 'open'")
                        .into_iter()
                        .filter(|t| features.iter().all(|(_, b)| t.contains(b.as_str())))
                        .collect();
                (!stacked.is_empty() || !items.is_empty()).then_some((stacked, items))
            });
            let (stacked, items) = decision;
            assert!(
                stacked.len() + items.len() == 1,
                "1 decision names both features: stacks {stacked:?}, needs-you items {items:?}"
            );
        },
    );
}
