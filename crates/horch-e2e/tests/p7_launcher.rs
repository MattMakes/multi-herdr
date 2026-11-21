#![cfg(unix)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use horch_core::fleetd::task_execution::adapters::task_files::{attempt_dir, write_attempt_files};
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;
use horch_core::procid;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, testing, Store};
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, Harness};
use rusqlite::params;
use serde_json::Value;

const FORBIDDEN: &str = "ANTHROPIC_API_KEY";

const PERSONA: &str = "opus";

const FAKE_FILE: &str = "fake-claude.txt";

const CHECK_ENV: &str = "check-env.txt";

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

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn put(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn toml_value<'a>(toml: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let header = format!("[{section}]");
    toml.lines()
        .skip_while(|l| l.trim() != header)
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim())
        })
}

fn claude_docs_fixture(fixture: &Path) {
    let toml =
        std::fs::read_to_string(repo_root().join("crates/agent-docs-sync/docs-sources.toml"))
            .unwrap();
    let unquote = |v: &str| v.trim_matches('"').to_string();
    let url = unquote(toml_value(&toml, "claude", "url").expect("the url of claude"));
    let suffix = toml_value(&toml, "claude", "md_suffix").map_or(String::new(), unquote);
    let include = toml_value(&toml, "claude", "include").expect("the pages of claude");
    let at = |url: &str| fixture.join(url.split_once("://").map_or(url, |(_, r)| r));
    let base = url.trim_end_matches("llms.txt");
    let mut index = String::from("# claude\n\n");
    for entry in include.trim_matches(['[', ']']).split(',') {
        let entry = unquote(entry.trim());
        let page = format!("{base}{entry}{suffix}");
        index.push_str(&format!("- [{entry}]({page}): {entry}.\n"));
        put(&at(&page), &format!("# {entry}\n\nUse `--flag-{entry}`.\n"));
    }
    put(&at(&url), &index);
}

struct World {
    h: Harness,
    agent: Child,
    t1: (String, PathBuf, PathBuf),
}

impl World {
    fn new(name: &str) -> World {
        let mut h = Harness::new(name).with_git();
        std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
        link_bwrap(&h);
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), "pass_period_s = 1\n").unwrap();
        let docs = h.root.join("docs-fixture");
        claude_docs_fixture(&docs);
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_WORKSPACE_ID", "w1");
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        h.set("HORCH_FAKE_SCENARIO", "exec");
        h.set(FORBIDDEN, "dummy-forbidden-value");
        declare_gates(&h);
        let agent = stand_in(&h);
        let check = format!(
            "env > '{}' && test -s {FAKE_FILE}",
            h.tmp.join(CHECK_ENV).display()
        );
        let t1 = seed(&h, &check, &agent);
        let mut create = Command::new(h.bin.join("herdr"));
        h.seal(&mut create);
        create.env_remove(FORBIDDEN);
        let label = h
            .project
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let o = create
            .args(["workspace", "create", "--label", &label])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
        h.serve_fleetd();
        World { h, agent, t1 }
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn t1_says_done(&mut self) {
        let (worker, yaml, tree) = self.t1.clone();
        let made = format!("made {}", tree.display());
        self.wait("the worktree_add job of t1 to pass", |w| {
            w.rows("SELECT result_json FROM jobs WHERE kind = 'worktree_add' AND state = 'passed'")
                .iter()
                .any(|r| r.contains(&made))
        });
        let keys = format!(
            "commit,result\n{worker}\n{}\n{}\n",
            yaml.display(),
            tree.display()
        );
        self.agent
            .stdin
            .take()
            .expect("the stdin of the wrapper")
            .write_all(keys.as_bytes())
            .unwrap();
        let result = yaml.with_file_name("outbox").join("result.json");
        self.wait("the fake claude of t1 to write result.json", |_| {
            result.is_file()
        });
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn ended(&self, kind: &str) -> bool {
        let subject = if kind == "task_check" {
            "%"
        } else {
            "claude %"
        };
        !self
            .rows(&format!(
                "SELECT state FROM jobs WHERE kind = '{kind}' AND subject LIKE '{subject}'
                 AND state NOT IN ('queued', 'running')"
            ))
            .is_empty()
    }

    fn exec_calls(&self) -> Vec<Value> {
        self.h
            .calls_of("claude")
            .into_iter()
            .filter(|c| c.get("exec_prompt").is_some())
            .collect()
    }

    fn is_conformance(call: &Value) -> bool {
        call["argv"].to_string().contains("/conformance-")
    }

    fn diagnostics(&self) -> String {
        let calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .filter(|c| c["fake"] != "herdr")
            .map(|c| {
                format!(
                    "{} {} commit={} result={} violations={}",
                    c["fake"], c["argv"], c["commit"], c["result"], c["violations"]
                )
            })
            .collect();
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        let mut files = Vec::new();
        let mut todo = vec![self.fleet_root().join("tasks")];
        while let Some(dir) = todo.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    todo.push(path);
                } else {
                    let rel = path.strip_prefix(self.fleet_root()).unwrap().display();
                    let json = path.extension().is_some_and(|e| e == "json");
                    let body = if json {
                        read(path.clone())
                    } else {
                        String::new()
                    };
                    files.push(format!("{rel} {body}"));
                }
            }
        }
        format!(
            "calls:\n{}\nfleetd.stderr:\n{}\ntask files:\n{}\njobs:\n{:?}\nattempts:\n{:?}",
            calls.join("\n"),
            read(self.h.root.join("fleetd.stderr")),
            files.join("\n"),
            self.rows(
                "SELECT kind || ' ' || subject || ' ' || state || ' run=' || run
                     || ' abandoned=' || abandoned_runs || ' ' || COALESCE(result_json, '')
                 FROM jobs"
            ),
            self.rows(
                "SELECT id || ' ' || via || ' ' || state || ' pid=' || COALESCE(pid, '-')
                 || ' outcome=' || COALESCE(outcome, '-') FROM attempts
                 UNION ALL SELECT 'task ' || status FROM tasks
                 UNION ALL SELECT 'ingested ' || path || ' ' || outcome || ' '
                     || COALESCE(reason, '') FROM ingested
                 UNION ALL SELECT 'event ' || kind || ' ' || substr(text, 1, 300)
                     FROM events WHERE task_id IS NOT NULL
                 UNION ALL SELECT 'needs_you ' || kind || ' ' || substr(text, 1, 300)
                     FROM needs_you"
            ),
        )
    }

    fn wait(&self, what: &str, mut ready: impl FnMut(&World) -> bool) {
        wait_for_or(what, || ready(self).then_some(()), || self.diagnostics());
    }
}

fn declare_gates(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
        &["branch", "fleet/r1", "main"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}

fn stand_in(h: &Harness) -> Child {
    let mut cmd = Command::new("/bin/sh");
    h.seal(&mut cmd);
    cmd.env_remove(FORBIDDEN);
    cmd.args([
        "-c",
        "read s; read w; read y; read d; cd \"$d\"; \
         HORCH_FAKE_SCENARIO=\"$s\" HORCH_WORKER_ID=\"$w\" HORCH_TASK_YAML=\"$y\" \"$@\"; \
         exec sleep 600",
        "sh",
    ])
    .arg(h.bin.join("claude"))
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap()
}

fn seed(h: &Harness, check: &str, agent: &Child) -> (String, PathBuf, PathBuf) {
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            run_branch: Some("fleet/r1".into()),
            workspace_id: Some("w1".into()),
            finish_steps: VerifyProfile::Minimal.steps(),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let seat = workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            pane_id: Some("w1:p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    runs::set_orchestrator(conn, &run.run_id, &seat).unwrap();
    add_task(h, conn, &run, "t0", None);
    let t1 = add_task(h, conn, &run, "t1", Some(check));
    let pid = agent.id();
    let started = procid::start_time(pid).expect("the start time of the wrapper");
    let bound = testing::bind_live(
        conn,
        &t1,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: PERSONA.into(),
            ..Default::default()
        },
        Some(Recorded {
            pid,
            pid_start: Some(started),
        }),
    )
    .unwrap();
    let set = conn
        .execute(
            "UPDATE attempts SET via = 'push', pid = ?2, pid_start = ?3 WHERE id = ?1",
            params![
                bound.attempt_id,
                i64::from(pid),
                i64::try_from(started).unwrap()
            ],
        )
        .unwrap();
    assert_eq!(set, 1, "the bind writes the attempt {}", bound.attempt_id);
    let tree = bound.worktree.expect("a worktree task binds a worktree");
    let facts = TaskFacts {
        task: "T2".into(),
        task_id: t1.clone(),
        run: "r1".into(),
        attempt: "A1".into(),
        profile: PERSONA.into(),
        worktree: tree.path.clone(),
        branch: tree.branch.clone(),
        read_first: "tickets/t1.md".into(),
        goal: "t1".into(),
        check: Some(check.into()),
        kind: Some(WorkKind::Feature.as_str().into()),
        ticket_snapshot: "# t1\n".into(),
        extra: None,
        handoff: None,
        idea: None,
        feature: None,
        covers: Vec::new(),
        verdict: None,
    };
    let fleet = h.root.join("fleet");
    write_attempt_files(&fleet, &facts, &[]).unwrap();
    let yaml = attempt_dir(&fleet, &t1, "A1").join("task.yaml");
    (bound.worker_id, yaml, PathBuf::from(tree.path))
}

fn add_task(
    h: &Harness,
    conn: &rusqlite::Connection,
    run: &Run,
    name: &str,
    check: Option<&str>,
) -> String {
    let ticket = format!("# {name}\nDo the work of {name}.\n");
    put(&h.project.join(format!("tickets/{name}.md")), &ticket);
    let mut new = NewTask::work(name, &format!("tickets/{name}.md"), PERSONA);
    new.ticket_snapshot = Some(ticket);
    new.land_check = check.map(str::to_string);
    new.work_kind = Some(WorkKind::Feature);
    tasks::add(conn, run, &new, Actor::Orchestrator)
        .unwrap()
        .task_id
}

fn assert_no_call_has_the_name(w: &World) {
    let calls = w.h.calls();
    assert!(!calls.is_empty());
    for call in &calls {
        let keys = call["env_keys"].as_array().expect("each call has env_keys");
        assert!(
            !keys.iter().any(|k| k == FORBIDDEN),
            "a child has {FORBIDDEN}: {} {}",
            call["fake"],
            call["argv"]
        );
    }
}

#[test]
fn no_child_of_a_worker_job_check_docs_sync_or_conformance_has_the_forbidden_key() {
    horch_e2e::expected::check(
        file!(),
        "no_child_of_a_worker_job_check_docs_sync_or_conformance_has_the_forbidden_key",
        || {
            let mut w = World::new("p7-launcher");

            w.t1_says_done();
            let check_env = w.h.tmp.join(CHECK_ENV);
            w.wait("the task_check job to end", |w| {
                w.ended("task_check") && check_env.is_file()
            });
            let env = std::fs::read_to_string(&check_env).unwrap();
            assert!(
                !env.lines().any(|l| l.starts_with(&format!("{FORBIDDEN}="))),
                "the check of the task has {FORBIDDEN}"
            );
            w.wait("the docs_sync job of claude to end", |w| {
                w.ended("docs_sync")
            });
            let versions = std::fs::read_dir(w.fleet_root().join("docs/claude"))
                .map(|d| d.flatten().filter(|e| e.path().is_dir()).count())
                .unwrap_or(0);
            assert!(
                versions > 0,
                "docs sync wrote no version folder of claude: {}",
                w.diagnostics()
            );
            assert!(
                w.h.calls_of("claude")
                    .iter()
                    .any(|c| c["argv"] == serde_json::json!(["--version"])),
                "no version call of the fake claude: {}",
                w.diagnostics()
            );
            w.wait("the conformance job of claude to end", |w| {
                w.ended("conformance")
            });
            assert!(
                w.exec_calls().iter().any(World::is_conformance),
                "no conformance launch of the fake claude: {}",
                w.diagnostics()
            );
            assert_no_call_has_the_name(&w);

            w.wait("the fake agent of the worker attempt of t0", |w| {
                w.exec_calls().iter().any(|c| !World::is_conformance(c))
            });
            assert_no_call_has_the_name(&w);
        },
    );
}
