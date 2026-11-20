#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};

const FLEET_TOML: &str = "pass_period_s = 1\n";

const PROJECT_FLEET_TOML: &str = "[gates]\nfast = \"true\"\nfull = \"true\"\n";

const TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** docs\n\
**Review:** light\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

const ESCALATED_KIND: &str = "judge_escalated";

const JUDGE_CANCEL_REFUSED: &str = "its judge follows";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[derive(Debug, Clone)]
struct Pair {
    judged: String,
    judged_short: String,
    judge: String,
    run: String,
    idea: String,
}

struct World {
    h: Harness,
}

impl World {
    fn new(name: &str, scenario: &str) -> World {
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
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
        let ws = h.fleet_workspace();
        h.set("HORCH_WORKSPACE_ID", ws);

        let tickets = h.project.join("tickets");
        std::fs::create_dir_all(&tickets).unwrap();
        let ticket = tickets.join("01-queue-guide.md");
        std::fs::write(&ticket, TICKET).unwrap();
        std::fs::write(h.project.join("fleet.toml"), PROJECT_FLEET_TOML).unwrap();
        let w = World { h };
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);

        w.h.serve_fleetd();
        for cli in ["claude", "codex", "agy"] {
            wait_for_or(
                &format!("an ended conformance run of {cli}"),
                || {
                    let ended = w.rows(&format!(
                        "SELECT id FROM jobs WHERE kind = 'conformance' \
                         AND subject LIKE '{cli} %' \
                         AND state IN ('passed', 'failed', 'abandoned')"
                    ));
                    (!ended.is_empty()).then_some(())
                },
                || w.context(),
            );
        }
        let o =
            w.h.run(&["idea", "add", "--direct", &ticket.to_string_lossy()]);
        assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
        w
    }

    fn git(&self, args: &[&str]) {
        let o = self
            .h
            .git_cmd(args)
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }

    fn ledger(&self) -> PathBuf {
        self.h.ledger_db()
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let Ok(conn) = Connection::open_with_flags(self.ledger(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
            return Vec::new();
        };
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        let Ok(mut stmt) = conn.prepare(sql) else {
            return Vec::new();
        };
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            let mut cols = Vec::new();
            for i in 0..n {
                let v: rusqlite::types::Value = r.get(i)?;
                cols.push(match v {
                    rusqlite::types::Value::Null => "-".to_string(),
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Real(f) => f.to_string(),
                    rusqlite::types::Value::Text(t) => t,
                    rusqlite::types::Value::Blob(_) => "<blob>".to_string(),
                });
            }
            Ok(cols.join(" "))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn one(&self, sql: &str) -> Option<String> {
        self.rows(sql).into_iter().next()
    }

    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        [
            show("now", "SELECT strftime('%H:%M:%f', 'now')"),
            show(
                "tasks",
                "SELECT task_id, kind, IFNULL(parent_task_id, '-'), status, \
                 fix_rounds, attempt_floor, IFNULL(idea_id, '-'), \
                 IFNULL(primary_persona, '-') FROM tasks",
            ),
            show(
                "profile facts",
                "SELECT event_id, substr(at, 12, 12), entity_id, kind, IFNULL(cause, '-') \
                 FROM events WHERE entity = 'profile' ORDER BY event_id",
            ),
            show(
                "attempts",
                "SELECT id, IFNULL(task_id, '-'), state, IFNULL(outcome, '-') FROM attempts",
            ),
            show("needs_you", "SELECT key, kind, state, text FROM needs_you"),
            show(
                "jobs",
                "SELECT id, kind, subject, state, substr(IFNULL(result_json, '-'), 1, 300) \
                 FROM jobs WHERE kind <> 'conformance' ORDER BY id",
            ),
            show(
                "task facts",
                "SELECT event_id, substr(at, 12, 12), entity_id, kind, substr(IFNULL(cause, '-'), 1, 300), \
                 substr(IFNULL(data_json, '-'), 1, 400) \
                 FROM events WHERE entity = 'task' AND kind <> 'transition' ORDER BY event_id",
            ),
            show(
                "attempt facts",
                "SELECT event_id, substr(at, 12, 12), entity_id, kind, IFNULL(from_state, '-'), \
                 IFNULL(to_state, '-'), substr(IFNULL(cause, '-'), 1, 200) \
                 FROM events WHERE entity = 'attempt' ORDER BY event_id",
            ),
            show(
                "ideas",
                "SELECT id, state, IFNULL(plan_sha256, '-') FROM ideas",
            ),
            show(
                "task moves",
                "SELECT event_id, substr(at, 12, 12), entity_id, from_state, to_state, IFNULL(cause, '-') \
                 FROM events WHERE kind = 'transition' AND entity = 'task' ORDER BY event_id",
            ),
            format!("git refs:\n{}\n", self.refs()),
            format!("idea folder files:\n{}\n", self.idea_files()),
            format!(
                "fleetd stderr, each line 1 time:\n{}\n",
                self.fleetd_stderr()
            ),
        ]
        .concat()
    }

    fn refs(&self) -> String {
        let o = self
            .h
            .git_cmd(&["for-each-ref", "--format=%(refname) %(objectname:short)"])
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn idea_files(&self) -> String {
        let dir = self.h.fleet_root().join("ideas/I-0001");
        let mut out = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p.strip_prefix(&dir).unwrap_or(&p).display().to_string());
                }
            }
        }
        out.sort();
        let verdict = std::fs::read_to_string(dir.join("plan.verdict.json")).unwrap_or_default();
        format!("  {}\nplan.verdict.json: {verdict}", out.join("\n  "))
    }

    fn fleetd_stderr(&self) -> String {
        let all = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        all.lines()
            .filter(|l| seen.insert(*l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }

    fn pair(&self) -> Pair {
        self.wait("the judged task and its judge task", || {
            let row = self.one(
                "SELECT w.task_id, 'T-' || w.seq, j.task_id, j.run_id, j.idea_id \
                 FROM tasks j JOIN tasks w ON w.task_id = j.parent_task_id \
                 WHERE j.idea_id IS NOT NULL AND w.idea_id = j.idea_id",
            )?;
            let mut it = row.split(' ').map(str::to_string);
            Some(Pair {
                judged: it.next()?,
                judged_short: it.next()?,
                judge: it.next()?,
                run: it.next()?,
                idea: it.next()?,
            })
        })
    }

    fn status(&self, task: &str) -> String {
        self.one(&format!(
            "SELECT status FROM tasks WHERE task_id = '{task}'"
        ))
        .unwrap_or_default()
    }

    fn wait_status(&self, task: &str, status: &str) {
        self.wait(&format!("task {task} in {status}"), || {
            (self.status(task) == status).then_some(())
        });
    }

    fn moved_at(&self, task: &str, to: &str) -> Option<i64> {
        self.one(&format!(
            "SELECT MIN(event_id) FROM events WHERE kind = 'transition' \
             AND entity = 'task' AND entity_id = '{task}' AND to_state = '{to}'"
        ))
        .and_then(|v| v.parse().ok())
    }

    fn attempts_with(&self, task: &str, outcome: &str) -> usize {
        self.one(&format!(
            "SELECT COUNT(*) FROM attempts WHERE task_id = '{task}' AND outcome = '{outcome}'"
        ))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
    }

    fn plan(&self, pair: &Pair) -> (String, Vec<String>) {
        let idea = &pair.idea;
        let sha = self
            .one(&format!(
                "SELECT IFNULL(plan_sha256, '-') FROM ideas WHERE id = '{idea}'"
            ))
            .unwrap_or_default();
        let tasks = self.rows(&format!(
            "SELECT task_id FROM tasks WHERE idea_id = '{idea}' ORDER BY task_id"
        ));
        (sha, tasks)
    }

    fn moved(&self, task: &str, from: &str, to: &str) -> bool {
        self.one(&format!(
            "SELECT 1 FROM events WHERE kind = 'transition' AND entity = 'task' \
             AND entity_id = '{task}' AND from_state = '{from}' AND to_state = '{to}'"
        ))
        .is_some()
    }

    fn cancel(&self, task: &str, run: &str) -> Output {
        self.h
            .run(&["task", "cancel", task, "the test cancels it", "--run", run])
    }
}

#[test]
fn rr3_judgment_and_judge_reach_done_with_no_plan_change() {
    let w = World::new("p6-judge-done", "commit,result,verdict_pass");
    let pair = w.pair();
    let planned = w.plan(&pair);
    w.wait_status(&pair.judge, "done");
    w.wait_status(&pair.judged, "done");
    assert_eq!(
        w.plan(&pair),
        planned,
        "the plan does not change\n{}",
        w.context()
    );
    let fix_rounds = w.one(&format!(
        "SELECT fix_rounds FROM tasks WHERE task_id = '{}'",
        pair.judged
    ));
    assert_eq!(fix_rounds.as_deref(), Some("0"), "{}", w.context());
}

#[test]
fn rr3_critical_gives_1_fix_round() {
    let w = World::new("p6-judge-critical", "commit,result,verdict_critical");
    let pair = w.pair();
    let snapshot = |w: &World| {
        w.one(&format!(
            "SELECT t.fix_rounds, t.attempt_floor, \
             (SELECT COUNT(*) FROM attempts a WHERE a.task_id = '{judge}' \
              AND a.outcome = 'critical_open') \
             FROM tasks t WHERE t.task_id = '{judged}'",
            judge = pair.judge,
            judged = pair.judged,
        ))
    };
    let first = w.wait("the first fix round", || {
        let s = snapshot(&w)?;
        (s.split(' ').next() != Some("0")).then_some(s)
    });
    let cols: Vec<&str> = first.split(' ').collect();
    let (rounds, floor, critical) = (cols[0], cols[1], cols[2]);
    assert_eq!(
        rounds,
        critical,
        "each critical_open judge attempt gives 1 fix round\n{}",
        w.context()
    );
    assert_eq!(
        floor,
        rounds,
        "a fix attempt takes no attempt of max_attempts\n{}",
        w.context()
    );
    assert!(
        w.moved(&pair.judged, "verifying", "ready") && w.moved(&pair.judge, "verifying", "pending"),
        "the judged task gets a fix attempt and the judge task waits\n{}",
        w.context()
    );
    assert_ne!(w.status(&pair.judged), "done", "{}", w.context());
    assert_ne!(w.status(&pair.judge), "done", "{}", w.context());
}

#[test]
fn fm9_judgment_task_is_done_only_after_its_judge() {
    let w = World::new("p6-judge-fm9", "commit,result,verdict_pass");
    let pair = w.pair();
    w.wait_status(&pair.judged, "done");
    let judged_done = w.moved_at(&pair.judged, "done").unwrap();
    let judge_done = w.moved_at(&pair.judge, "done");
    assert!(
        judge_done.is_some_and(|j| j < judged_done),
        "the judge task is done before the judged task\n{}",
        w.context()
    );
    let verifying = w.moved_at(&pair.judged, "verifying").unwrap();
    let judge_ready = w.moved_at(&pair.judge, "ready").unwrap_or(i64::MAX);
    assert!(
        verifying < judge_ready,
        "the judge task is ready only after the judged task is in verifying\n{}",
        w.context()
    );
    let moves = w.rows(&format!(
        "SELECT from_state || '>' || to_state FROM events WHERE kind = 'transition' \
         AND entity = 'task' AND entity_id = '{}' AND event_id > {verifying} \
         AND event_id < {judged_done}",
        pair.judged
    ));
    assert!(
        moves.is_empty(),
        "the judged task waits in verifying until it is done: {moves:?}\n{}",
        w.context()
    );
}

#[test]
fn fm33_two_fix_rounds_escalate() {
    let w = World::new("p6-judge-fm33", "commit,result,verdict_critical");
    let pair = w.pair();
    let attempts = |task: &str| {
        w.one(&format!(
            "SELECT COUNT(*) FROM attempts WHERE task_id = '{task}'"
        ))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
    };
    for n in 1..=3 {
        w.wait(&format!("judge attempt {n} bound"), || {
            (attempts(&pair.judge) >= n).then_some(())
        });
        w.wait(
            &format!("judge attempt {n} with an open CRITICAL item"),
            || (w.attempts_with(&pair.judge, "critical_open") >= n).then_some(()),
        );
    }
    w.wait_status(&pair.judged, "escalated");
    w.wait_status(&pair.judge, "escalated");
    let rounds = w.one(&format!(
        "SELECT fix_rounds FROM tasks WHERE task_id = '{}'",
        pair.judged
    ));
    assert_eq!(rounds.as_deref(), Some("2"), "{}", w.context());
    assert_eq!(
        w.attempts_with(&pair.judge, "critical_open"),
        3,
        "3 judge attempts with an open CRITICAL item\n{}",
        w.context()
    );
    let items = w.rows(&format!(
        "SELECT key FROM needs_you WHERE state = 'open' AND kind = '{ESCALATED_KIND}'"
    ));
    assert_eq!(
        items,
        vec![format!("{ESCALATED_KIND}:{}", pair.judged)],
        "1 needs-you item\n{}",
        w.context()
    );
    assert!(
        w.moved_at(&pair.judged, "done").is_none() && w.moved_at(&pair.judge, "done").is_none(),
        "neither task was done\n{}",
        w.context()
    );
}

#[test]
fn cancel_of_a_judged_task_cancels_its_judge_task() {
    let w = World::new("p6-judge-cancel", "stay");
    let pair = w.pair();
    let o = w.cancel(&pair.judged, &pair.run);
    assert!(o.status.success(), "horch task cancel: {}", text(&o));
    w.wait_status(&pair.judged, "cancelled");
    w.wait_status(&pair.judge, "cancelled");
}

#[test]
fn cancel_of_a_judge_task_alone_is_refused() {
    let w = World::new("p6-judge-refused", "stay");
    let pair = w.pair();
    let o = w.cancel(&pair.judge, &pair.run);
    let said = text(&o);
    assert!(!o.status.success(), "the cancel is refused: {said}");
    assert!(
        said.contains(JUDGE_CANCEL_REFUSED) && said.contains(&pair.judged_short),
        "the refusal names the judged task: {said}"
    );
    assert_ne!(w.status(&pair.judge), "cancelled", "{}", w.context());
    assert_ne!(w.status(&pair.judged), "cancelled", "{}", w.context());
    assert!(
        w.one(&format!(
            "SELECT 1 FROM tasks WHERE task_id = '{}' AND cancel_requested IS NOT NULL",
            pair.judge
        ))
        .is_none(),
        "no cancel is recorded on the judge task\n{}",
        w.context()
    );
}
