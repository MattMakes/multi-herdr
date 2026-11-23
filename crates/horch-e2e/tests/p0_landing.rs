#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant, SystemTime};

use horch_e2e::harness::{fixtures, wait_for_or, Harness};

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

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.serve_fleetd();
    h
}

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

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

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

fn git_commit_all(h: &Harness, message: &str) {
    let project = h.project.clone();
    git_in(h, &project, &["add", "-A"]);
    git_in(
        h,
        &project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", message],
    );
}

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

fn declare_gates(h: &Harness, fast: &str) {
    let fast = fast.replace('\\', "\\\\").replace('"', "\\\"");
    std::fs::write(
        h.project.join("fleet.toml"),
        format!("[gates]\nfast = \"{fast}\"\nfull = \"true\"\n"),
    )
    .unwrap();
}

fn pass(h: &Harness, before: impl FnOnce()) {
    let mut restarted = SystemTime::now();
    h.without_fleetd(|| {
        before();
        restarted = SystemTime::now();
    });
    wait_for_or(
        "the pass of the new fleetd",
        || pass_ended_since(h, restarted).then_some(()),
        || evidence(h),
    );
    wait_for_or(
        "the spawn of each launch of the pass",
        || unrecorded_launches(h).is_empty().then_some(()),
        || evidence(h),
    );
}

fn pass_ended_since(h: &Harness, t: SystemTime) -> bool {
    let run = h.fleet_root().join("run");
    let mtime = |name: &str| {
        std::fs::metadata(run.join(name))
            .and_then(|m| m.modified())
            .ok()
    };
    match (mtime("activity.json"), mtime("heartbeat.json")) {
        (Some(activity), Some(heartbeat)) => activity >= t && heartbeat >= activity,
        _ => false,
    }
}

fn unrecorded_launches(h: &Harness) -> Vec<String> {
    let launches = h.fleet_root().join("run").join("launches");
    ledger_rows(
        h,
        "SELECT a.id FROM attempts a JOIN workers w ON w.worker_id = a.worker_id \
         WHERE a.state = 'launching' AND a.task_id IS NOT NULL AND w.pane_id IS NULL \
         AND w.status = 'spawning'",
    )
    .into_iter()
    .filter(|id| {
        launches
            .join(format!("{}.json", id.replace('/', "%2F")))
            .is_file()
    })
    .collect()
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

fn files_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.file_name().is_some_and(|n| n == name) {
                found.push(p);
            }
        }
    }
    found
}

fn yaml_key(y: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    y.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|v| v.trim().trim_matches('"').to_string())
}

fn queue_run(h: &Harness, workspace: &str, tickets: &[&str], check: &str) {
    use horch_core::taskledger::model::{Actor, VerifyProfile};
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::{schema, Store};

    let project = h.project.clone();
    git_in(h, &project, &["branch", "fleet/alpha", "main"]);
    let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "alpha".into(),
            workspace_id: Some(workspace.into()),
            base_branch: Some("main".into()),
            run_branch: Some("fleet/alpha".into()),
            finish_steps: VerifyProfile::Minimal.steps(),
            ..Default::default()
        },
    )
    .unwrap();
    let now = horch_core::clock::now_stamp();
    schema::run_idea(conn, &run.run_id, &now, true)
        .unwrap()
        .expect("the idea of the run");
    for name in tickets {
        let p = ticket(h, name, &format!("# {name}\n"));
        let mut new = NewTask::work(name, &p.to_string_lossy(), "opus");
        new.land_check = Some(check.into());
        tasks::add(conn, &run, &new, Actor::Orchestrator).unwrap();
    }
}

fn jobs_of(h: &Harness, kind: &str) -> Vec<(i64, i64, String)> {
    ledger_rows(
        h,
        &format!(
            "SELECT id || ' ' || run || ' ' || state FROM jobs WHERE kind = '{kind}' ORDER BY id"
        ),
    )
    .iter()
    .map(|r| {
        let mut f = r.split(' ');
        let mut num = || f.next().unwrap().parse::<i64>().unwrap();
        let (id, run) = (num(), num());
        (id, run, f.next().unwrap().to_string())
    })
    .collect()
}

fn wait_jobs_ended(h: &Harness, kind: &str) {
    let fleet_root = h.fleet_root();
    wait_for_or(
        &format!("the result files of the running {kind} jobs"),
        || {
            jobs_of(h, kind)
                .iter()
                .filter(|(_, _, state)| state == "running")
                .all(|(id, run, _)| {
                    horch_core::fleetd::fleet_runtime::adapters::job_result_file::path(
                        &fleet_root,
                        *id,
                        *run,
                    )
                    .is_file()
                })
                .then_some(())
        },
        || evidence(h),
    );
}

fn evidence(h: &Harness) -> String {
    [
        "SELECT 'task ' || task_id || ' ' || status FROM tasks",
        "SELECT 'attempt ' || id || ' ' || state || ' pid ' || ifnull(pid, '-') || ' via ' || ifnull(via, '-') FROM attempts",
        "SELECT 'job ' || id || ' ' || kind || ' ' || state FROM jobs",
        "SELECT 'branch ' || name || ' ' || ifnull(landing_state, '-') FROM branches",
        "SELECT 'bundle ' || id || ' ' || state FROM bundles",
        "SELECT 'item ' || key || ' ' || state || ' ' || text FROM needs_you",
        "SELECT 'worker ' || worker_id || ' ' || status || ' pane ' || ifnull(pane_id, '-') FROM workers",
    ]
    .iter()
    .flat_map(|q| ledger_rows(h, q))
    .chain(
        h.calls_of("claude")
            .iter()
            .map(|c| format!("claude call: {}", c["result"])),
    )
    .chain(
        std::fs::read_dir(h.fleet_root().join("run").join("launches"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| format!("launch record: {}", e.file_name().to_string_lossy())),
    )
    .chain(
        std::fs::read_to_string(h.fleet_root().join("run").join("launch.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| format!("launch.log: {l}")),
    )
    .chain(
        std::fs::read_to_string(h.root.join("fleetd.stderr"))
            .unwrap_or_default()
            .lines()
            .map(|l| format!("fleetd: {l}")),
    )
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten() {
    horch_e2e::expected::check(
        file!(),
        "given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten",
        || {
            let mut h = Harness::new("p0-bundle").with_git();
            let db = h.root.join("ledger.sqlite3");
            h.set("HORCH_LEDGER_DB", db.to_string_lossy());
            h.set("HORCH_NO_WAKE", "1");
            h.set("HORCH_NO_WATCHDOG", "1");
            h.set("HORCH_FAULT", "drop-watch-events");
            h.set(
                "HORCH_QUOTA_FILE",
                fixtures().join("quota/all-ok.json").to_string_lossy(),
            );
            let docs = h.root.join("docs-fixture");
            std::fs::create_dir_all(&docs).unwrap();
            h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
            h.set("HORCH_FAKE_SCENARIO", "exec,result");
            std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
            link_bwrap(&h);
            let workspace = h.fleet_workspace();
            h.set("HORCH_WORKSPACE_ID", &workspace);
            let fleet = h.fleet_root();
            std::fs::create_dir_all(&fleet).unwrap();
            std::fs::write(fleet.join("fleet.toml"), "pass_period_s = 3600\n").unwrap();

            declare_gates(&h, "echo the fast gate ran");
            git_commit_all(&h, "gates");
            add_bare_remote(&h);
            let go = h.root.join("go");
            let check = format!("while [ ! -e '{}' ]; do /bin/sleep 0.1; done", go.display());
            queue_run(&h, &workspace, &["01-a", "02-b", "03-c"], &check);
            h.serve_fleetd();
            let file_of = |task: &str| match task {
                "T1" => "a.txt",
                "T2" => "b.txt",
                "T3" => "c.txt",
                other => panic!("no file for task {other}"),
            };

            let tasks_dir = h.fleet_root().join("tasks");
            let committed = std::cell::RefCell::new(BTreeSet::new());
            let commit_ready = || {
                wait_jobs_ended(&h, "worktree_add");
                for yaml in files_named(&tasks_dir, "task.yaml") {
                    let y = std::fs::read_to_string(&yaml).unwrap_or_default();
                    let (Some(task), Some(tree)) = (yaml_key(&y, "task"), yaml_key(&y, "worktree"))
                    else {
                        continue;
                    };
                    let tree = PathBuf::from(tree);
                    if committed.borrow().contains(&task) || !tree.join("fleet.toml").is_file() {
                        continue;
                    }
                    commit_file(&h, &tree, file_of(&task));
                    committed.borrow_mut().insert(task);
                }
            };
            wait_for_or(
                "3 running task_check jobs",
                || {
                    pass(&h, commit_ready);
                    (jobs_of(&h, "task_check")
                        .iter()
                        .filter(|(_, _, state)| state == "running")
                        .count()
                        == 3)
                        .then_some(())
                },
                || evidence(&h),
            );
            let committed = committed.into_inner();
            assert_eq!(committed.len(), 3, "{committed:?}");

            pass(&h, || {
                std::fs::write(&go, "").unwrap();
                wait_jobs_ended(&h, "task_check");
            });

            let project = h.project.clone();
            let log = wait_for_or(
                "the 3 files on the run branch",
                || {
                    pass(&h, || ());
                    let log = git_in(&h, &project, &["log", "--format=%s", "fleet/alpha"]);
                    let landed: BTreeSet<&str> = log.lines().collect();
                    ["a.txt", "b.txt", "c.txt"]
                        .iter()
                        .all(|f| landed.contains(f))
                        .then_some(log.clone())
                },
                || evidence(&h),
            );
            let landed: BTreeSet<&str> = log.lines().collect();
            for file in ["a.txt", "b.txt", "c.txt"] {
                assert!(
                    landed.contains(file),
                    "the run branch has no commit {file}: {log}"
                );
            }
            let outputs = h.fleet_root().join("gate-output");
            let runs = std::fs::read_dir(&outputs)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| {
                    std::fs::read_to_string(e.path())
                        .unwrap_or_default()
                        .contains("the fast gate ran")
                })
                .count();
            assert_eq!(
                runs,
                1,
                "the fast gate runs 1 time for the bundle of 3 ready branches; \
                 the fast gate ran {runs} times\nbundles: {:?}",
                ledger_rows(
                    &h,
                    "SELECT id || ' ' || state || ' ' || ifnull(parent_id, '-') FROM bundles"
                )
            );
        },
    );
}

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
            let c1 = |title: &str, own: &str| {
                format!(
                    "# {title}\n\n<!-- horch-ticket: 1 -->\n\
                     **Persona:** primary `opus` · **Parallel:** yes\n\
                     **Kind:** feature\n\
                     **Check:** `true`\n\
                     **Own:** `{own}`\n\n\
                     ## Goal\n\n{title}.\n\n\
                     ## Steps\n\n1. {title}.\n\n\
                     ## Done when\n\nThe Check passes.\n"
                )
            };
            let retry_ticket = ticket(&h, "01-retry", &c1("01: Retry sync", "src/retry.rs"));
            let log_ticket = ticket(&h, "02-log", &c1("02: Log retries", "src/log.rs"));
            git_commit_all(&h, "base");
            add_bare_remote(&h);

            pass(&h, || ());

            ok(
                &h,
                &["idea", "add", "--direct", &retry_ticket.to_string_lossy()],
            );
            ok(
                &h,
                &["idea", "add", "--direct", &log_ticket.to_string_lossy()],
            );
            let features: Vec<(String, String)> = wait_for("2 feature branches", || {
                pass(&h, || ());
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
                let refs = rows.iter().all(|(_, b)| {
                    h.git_cmd(&[
                        "rev-parse",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{b}"),
                    ])
                    .current_dir(&project)
                    .output()
                    .unwrap()
                    .status
                    .success()
                });
                (rows.len() >= 2 && refs).then_some(rows)
            });
            assert_eq!(features.len(), 2, "2 ideas make 2 features: {features:?}");

            let tips: Vec<String> = features
                .iter()
                .map(|(_, b)| commit_on_branch(&h, b, "shared.txt", &format!("{b}\n")))
                .collect();

            let conflict = wait_for("a trial merge that conflicts", || {
                pass(&h, || ());
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

            let ids: BTreeSet<&str> = features.iter().map(|(id, _)| id.as_str()).collect();
            let decision = wait_for("a decision on the conflict", || {
                pass(&h, || ());
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
