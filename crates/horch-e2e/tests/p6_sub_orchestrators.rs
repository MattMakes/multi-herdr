//! The e2e tests of the dispatcher and the sub-orchestrators through the
//! binary (`docs/tickets/fleetd-p6/57-e2e-sub-orch.md`;
//! `docs/specs/fleetd/design.md`: FR-2, FM-16, FM-29 and FM-31).

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::ideas::application::sub_orchestrators::IDLE_STOP;
use horch_core::fleetd::ideas::application::waiting::{parked_key, waiting_key};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const IDLE_STOP_S: i64 = 30 * 60;

const MONDAY_IN_WORKING_HOURS: &str = "2030-01-07T10:00:00Z";

const AFTER_PARK: &str = "2030-01-15T10:00:00Z";

const FIRST_ANSWER: &str = "Which file does the idea change?\n\nAnswer: fake.txt";

const SECOND_ANSWER: &str = "How many tasks does it need?\n\nAnswer: 1 task";

fn ticket(n: u32, title: &str) -> String {
    format!(
        "# {n:02}: {title}\n\n\
         <!-- horch-ticket: 1 -->\n\
         **Persona:** primary `opus` · **Parallel:** yes\n\
         **Kind:** feature\n\
         **Review:** none\n\
         **Check:** `true`\n\
         **Own:** `fake-claude.txt`\n\n\
         ## Goal\n\n{title}.\n"
    )
}

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
}

impl World {
    fn new(name: &str, scenario: &str, before: impl FnOnce(&mut Harness)) -> World {
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
        std::fs::write(
            h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        before(&mut h);
        let w = World { h };
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);
        w.h.serve_fleetd();
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

    fn fleet_root(&self) -> PathBuf {
        self.h.fleet_root()
    }

    fn add(&self, args: &[&str]) -> String {
        let mut argv = vec!["idea", "add"];
        argv.extend_from_slice(args);
        let o = self.h.run(&argv);
        assert!(o.status.success(), "horch {}: {}", argv.join(" "), text(&o));
        let out = String::from_utf8_lossy(&o.stdout);
        out.split(|c: char| c.is_whitespace() || c == ':' || c == ',')
            .find(|w| w.starts_with("I-"))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("no idea id in: {}", text(&o)))
    }

    fn add_direct(&self, name: &str) -> String {
        let ticket = self.h.project.join("tickets").join(name);
        self.add(&["--direct", &ticket.to_string_lossy()])
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let Ok(conn) =
            Connection::open_with_flags(self.h.ledger_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)
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

    fn fleet_clock(&self) -> i64 {
        self.one("SELECT fleet_clock_s FROM fleetd WHERE id = 1")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    fn two_passes(&self) {
        let from = self.fleet_clock();
        self.wait("2 more fleetd passes", || {
            (self.fleet_clock() >= from + 2).then_some(())
        });
    }

    fn lead_attempts(&self) -> Vec<String> {
        self.rows(
            "SELECT id, IFNULL(idea_id, '-'), state, IFNULL(outcome, '-') FROM attempts
             WHERE task_id IS NULL ORDER BY rowid",
        )
    }

    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let fleetd = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        let fleetd: Vec<&str> = fleetd.lines().filter(|l| seen.insert(*l)).collect();
        [
            show(
                "features",
                "SELECT id, IFNULL(branch, '-'), state FROM features",
            ),
            show(
                "ideas",
                "SELECT id, feature_id, state, IFNULL(waiting_on_you_since, '-') FROM ideas",
            ),
            show(
                "sub-orchestrator attempts",
                "SELECT id, IFNULL(idea_id, '-'), state, IFNULL(outcome, '-') FROM attempts
                 WHERE task_id IS NULL",
            ),
            show(
                "task attempts",
                "SELECT id, task_id, state, IFNULL(outcome, '-') FROM attempts
                 WHERE task_id IS NOT NULL",
            ),
            show(
                "tasks",
                "SELECT task_id, status, IFNULL(idea_id, '-') FROM tasks",
            ),
            show(
                "questions",
                "SELECT id, task_id, feature_id, owner, state FROM questions",
            ),
            show("needs-you items", "SELECT key, state, text FROM needs_you"),
            format!("fleet clock: {}\n", self.fleet_clock()),
            format!(
                "the ask and the result of each fake claude:\n{}\n",
                self.h
                    .calls_of("claude")
                    .iter()
                    .filter(|c| ["commit", "ask", "result", "notes"]
                        .iter()
                        .any(|k| c.get(k).is_some()))
                    .map(|c| {
                        format!(
                            "  commit: {} ask: {} result: {} notes: {}",
                            c["commit"], c["ask"], c["result"], c["notes"]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            format!("interview.md of each idea:\n{}\n", self.interviews()),
            format!("fleetd stderr, each line 1 time:\n{}\n", fleetd.join("\n")),
        ]
        .concat()
    }

    fn interviews(&self) -> String {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(self.fleet_root().join("ideas"))
            .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.sort();
        dirs.iter()
            .filter_map(|d| {
                let text = std::fs::read_to_string(d.join("interview.md")).ok()?;
                Some(format!("  {}:\n{text}", d.display()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }

    fn wait_state(&self, idea: &str, state: &str) {
        self.wait(&format!("idea {idea} in {state}"), || {
            (self
                .one(&format!("SELECT state FROM ideas WHERE id = '{idea}'"))
                .as_deref()
                == Some(state))
            .then_some(())
        });
    }

    fn feature_of(&self, idea: &str) -> String {
        self.wait(&format!("the feature of idea {idea}"), || {
            self.one(&format!("SELECT feature_id FROM ideas WHERE id = '{idea}'"))
        })
    }

    fn wait_live_lead(&self, idea: &str, phase: &str) -> String {
        self.wait(&format!("a live {phase} attempt of idea {idea}"), || {
            self.one(&format!(
                "SELECT id FROM attempts WHERE task_id IS NULL AND idea_id = '{idea}'
                   AND id LIKE '{idea}/{phase}/%' AND state = 'live'"
            ))
        })
    }

    fn claude_argvs(&self) -> Vec<Vec<String>> {
        self.h
            .calls_of("claude")
            .into_iter()
            .map(|c| argv(&c))
            .collect()
    }
}

fn argv(call: &Value) -> Vec<String> {
    call["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[test]
fn fr2_each_sub_orchestrator_attempt_has_1_idea() {
    let w = World::new("p6-fr2", "stay", |_| {});
    let first = w.add(&["Show the queue length in the status bar"]);
    let feature = w.feature_of(&first);
    w.wait_state(&first, "clarifying");
    w.wait_live_lead(&first, "clarifying");

    let second = w.add(&["--to", &feature, "Colour the queue length red"]);
    let other = w.add(&["Add a help page for the queue"]);
    assert_eq!(w.feature_of(&second), feature, "{}", w.context());
    let other_feature = w.feature_of(&other);
    assert_ne!(other_feature, feature, "{}", w.context());
    w.wait_live_lead(&other, "clarifying");
    w.two_passes();

    assert_eq!(
        w.one(&format!("SELECT state FROM ideas WHERE id = '{second}'"))
            .as_deref(),
        Some("captured"),
        "{}",
        w.context()
    );
    assert!(
        w.rows(&format!(
            "SELECT id FROM attempts WHERE idea_id = '{second}'"
        ))
        .is_empty(),
        "{}",
        w.context()
    );
    for f in [&feature, &other_feature] {
        let live = w.rows(&format!(
            "SELECT a.id FROM attempts a JOIN ideas i ON i.id = a.idea_id
             WHERE a.task_id IS NULL AND a.state = 'live' AND i.feature_id = '{f}'"
        ));
        assert_eq!(live.len(), 1, "feature {f}: {live:?}\n{}", w.context());
    }
    let attempts = w.lead_attempts();
    assert!(attempts.len() >= 2, "{}", w.context());
    for row in &attempts {
        let mut cols = row.split(' ');
        let (id, idea) = (cols.next().unwrap(), cols.next().unwrap());
        assert_ne!(idea, "-", "attempt {id} has no idea\n{}", w.context());
        assert!(id.starts_with(&format!("{idea}/")), "{row}");
    }
    let ideas = w.rows("SELECT id FROM ideas");
    for i in [&first, &second, &other] {
        assert!(ideas.contains(i), "idea {i} is not in {ideas:?}");
    }
    for argv in w.claude_argvs() {
        let Some(brief) = argv.iter().find(|a| a.contains("idea-lead of idea")) else {
            continue;
        };
        let named = ideas
            .iter()
            .filter(|i| {
                let folder = w.fleet_root().join("ideas").join(i.as_str());
                brief
                    .split_whitespace()
                    .any(|word| word == i.as_str() || word == folder.to_string_lossy())
            })
            .count();
        assert_eq!(named, 1, "the launch names {named} ideas: {brief}");
    }
}

#[test]
fn fm16_no_orchestrator_and_questions_stay_in_their_feature() {
    horch_e2e::expected::check(
        file!(),
        "fm16_no_orchestrator_and_questions_stay_in_their_feature",
        || {
            let w = World::new("p6-fm16", "commit,ask,stay", |h| {
                let tickets = h.project.join("tickets");
                std::fs::create_dir_all(&tickets).unwrap();
                std::fs::write(
                    tickets.join("01-queue.md"),
                    ticket(1, "Write the queue guide"),
                )
                .unwrap();
                std::fs::write(tickets.join("02-help.md"), ticket(2, "Write the help page"))
                    .unwrap();
            });
            let ideas = [w.add_direct("01-queue.md"), w.add_direct("02-help.md")];
            let features = ideas.clone().map(|i| w.feature_of(&i));
            assert_ne!(features[0], features[1], "{}", w.context());

            for f in &features {
                let owner = w.wait(&format!("a question of feature {f}"), || {
                    w.one(&format!(
                        "SELECT owner FROM questions WHERE feature_id = '{f}'"
                    ))
                });
                assert_eq!(owner, format!("idea-lead:{f}"), "{}", w.context());
            }
            let rung = |f: &str| {
                w.rows(&format!(
                    "SELECT e.data_json FROM events e JOIN attempts a ON a.id = e.entity_id
                 JOIN ideas i ON i.id = a.idea_id
                 WHERE e.entity = 'attempt' AND e.kind = 'doorbell' AND i.feature_id = '{f}'"
                ))
            };
            for f in &features {
                w.wait(
                    &format!("a doorbell of the sub-orchestrator of {f}"),
                    || (!rung(f).is_empty()).then_some(()),
                );
            }
            for f in &features {
                let own: Vec<String> = w.rows(&format!(
                    "SELECT id FROM questions WHERE feature_id = '{f}'"
                ));
                for data in rung(f) {
                    let data: Value = serde_json::from_str(&data).unwrap();
                    for q in data["question_ids"].as_array().into_iter().flatten() {
                        let q = q.as_str().unwrap_or_default().to_string();
                        assert!(
                            own.contains(&q),
                            "the sub-orchestrator of {f} got question {q} of another feature\n{}",
                            w.context()
                        );
                    }
                }
            }
            let seats = w.rows(
            "SELECT worker_id FROM workers WHERE persona = 'orchestrator' OR kind = 'orchestrator'
               OR worker_id = 'orchestrator'",
        );
            assert!(seats.is_empty(), "orchestrator workers: {seats:?}");
            for call in w.h.calls_of("herdr") {
                let argv = argv(&call);
                assert!(
                    !argv.iter().any(|a| a == "orchestrator"),
                    "a herdr call names an orchestrator pane: {argv:?}"
                );
            }
        },
    );
}

#[test]
fn fm29_idle_stop_and_park() {
    let mut w = World::new("p6-fm29", "clarify,stay", |h| {
        h.set("HORCH_NOW", MONDAY_IN_WORKING_HOURS);
    });
    let idea = w.add(&["Show the queue length in the status bar"]);
    w.wait_state(&idea, "awaiting_approval");
    let attempt = w.wait_live_lead(&idea, "clarifying");
    w.wait("the waiting needs-you item", || {
        w.one(&format!(
            "SELECT state FROM needs_you WHERE key = '{}' AND state = 'open'",
            waiting_key(&idea)
        ))
    });
    w.wait("the pause of the lease of the idle process", || {
        w.one(&format!(
            "SELECT event_id FROM events WHERE entity = 'idea' AND entity_id = '{idea}'
               AND text LIKE '%lease paused%'"
        ))
    });

    w.h.advance_fleet_clock(IDLE_STOP_S - 60);
    w.two_passes();
    assert_eq!(
        w.one(&format!(
            "SELECT state FROM attempts WHERE id = '{attempt}'"
        ))
        .as_deref(),
        Some("live"),
        "{}",
        w.context()
    );
    w.h.advance_fleet_clock(60);
    w.wait("the idle stop of the sub-orchestrator", || {
        w.one(&format!(
            "SELECT id FROM attempts WHERE id = '{attempt}' AND state = 'ended'
               AND outcome = '{IDLE_STOP}'"
        ))
    });
    assert_eq!(
        w.one(&format!("SELECT state FROM ideas WHERE id = '{idea}'"))
            .as_deref(),
        Some("awaiting_approval"),
        "{}",
        w.context()
    );

    w.h.stop_fleetd();
    w.h.set("HORCH_NOW", AFTER_PARK);
    w.h.serve_fleetd();
    w.wait_state(&idea, "parked");
    w.wait("the parked needs-you note", || {
        w.one(&format!(
            "SELECT text FROM needs_you WHERE key = '{}' AND state = 'open'",
            parked_key(&idea)
        ))
    });
    assert!(
        w.one(&format!(
            "SELECT waiting_on_you_since FROM ideas WHERE id = '{idea}'"
        ))
        .as_deref()
            == Some("-"),
        "{}",
        w.context()
    );
}

fn claude_dies_on_first_idea_launch(h: &mut Harness) {
    let real = std::fs::read_link(h.bin.join("claude"))
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_fake-claude")));
    let mark = h.root.join("claude-died");
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\n\
         case \"$*\" in *'/ideas/'*) idea=1 ;; *) idea= ;; esac\n\
         if [ -n \"$idea\" ] && [ ! -e '{mark}' ]; then\n\
         : > '{mark}'\n\
         HORCH_FAKE_SCENARIO=exec,die_mid_interview\n\
         export HORCH_FAKE_SCENARIO\n\
         fi\n\
         exec '{real}' \"$@\"\n",
        mark = mark.display(),
        real = real.display()
    );
    h.write_warmed("claude", script.as_bytes());
}

#[test]
fn fm31_killed_sub_orchestrator_resumes_from_its_folder() {
    let w = World::new("p6-fm31", "clarify", claude_dies_on_first_idea_launch);
    let idea = w.add(&["Show the queue length in the status bar"]);
    let folder = w.fleet_root().join("ideas").join(&idea);
    let interview = folder.join("interview.md");

    w.wait("the answer of the first process in interview.md", || {
        std::fs::read_to_string(&interview)
            .ok()
            .filter(|t| t.contains(FIRST_ANSWER))
    });
    w.wait_state(&idea, "awaiting_approval");
    let attempts = w.lead_attempts();
    let phases: Vec<&str> = attempts
        .iter()
        .filter_map(|r| r.split(' ').next())
        .filter(|id| id.starts_with(&format!("{idea}/")))
        .collect();
    assert!(
        phases.len() >= 2,
        "a fresh process after the death: {attempts:?}\n{}",
        w.context()
    );
    assert!(
        phases
            .iter()
            .all(|id| id.starts_with(&format!("{idea}/clarifying/"))),
        "each process of the idea is in the phase clarifying: {phases:?}"
    );
    assert!(folder.join("assumptions.md").is_file(), "{}", w.context());
    let text = w.wait("the answer of the fresh process in interview.md", || {
        std::fs::read_to_string(&interview)
            .ok()
            .filter(|t| t.contains(SECOND_ANSWER))
    });
    assert!(text.contains(&format!("## Q1\n{FIRST_ANSWER}")), "{text}");
    assert!(text.contains(&format!("## Q2\n{SECOND_ANSWER}")), "{text}");
    assert_eq!(text.matches("\n## Q").count(), 2, "{text}");
    let notes = std::fs::read_dir(folder.join("notes")).unwrap().count();
    assert_eq!(notes, 2, "1 note for each question\n{}", w.context());
}
