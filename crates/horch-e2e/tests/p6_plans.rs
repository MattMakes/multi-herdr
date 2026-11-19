//! The e2e tests of plans through the binary: FR-3, FR-4 and FM-10
//! (`docs/tickets/fleetd-p6/58-e2e-plans.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! idea-lead. `horch idea add` adds 1 idea. The fake idea-lead writes
//! `assumptions.md`, `spec.md` and `requirements.yaml` (R1 and R2), and the
//! test approves the assumptions with `horch idea approve`. In `ticketing`,
//! the plan comes from the fake (`ticket_cycle`), or the test writes it in
//! the idea folder as the sub-orchestrator does. fleetd validates it and
//! writes `plan.verdict.json`.
//!
//! Each test waits for an event of the ledger or of a file, never for a
//! time. Each test sets `HORCH_TEST_DOCS_FIXTURE` to an empty local folder,
//! so no test uses the network.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Output;

use horch_core::digest::sha256_bytes;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The keys of the fake idea-lead that bring an idea to `ticketing` with
/// no plan: the test writes the plan.
const TO_TICKETING: &str = "clarify,specify";

/// The text of the idea of each test.
const IDEA: &str = "Write 1 line in fake.txt.";

/// The mode of each file that the test writes in an idea folder.
const FILE_MODE: u32 = 0o600;

/// The mode of each folder that the test makes in an idea folder.
const DIR_MODE: u32 = 0o700;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A C1 ticket with the kind `kind` that owns `file`.
fn ticket(key: &str, kind: &str, file: &str) -> String {
    format!(
        "# {key}: Write {file}\n\n\
         <!-- horch-ticket: 1 -->\n\
         **Persona:** primary `opus` · **Parallel:** yes\n\
         **Kind:** {kind}\n\
         **Review:** light\n\
         **Check:** `true`\n\
         **Own:** `{file}`\n\n\
         ## Goal\n\nThe task writes 1 line in {file}.\n"
    )
}

/// 1 task of plan.yaml.
struct Task<'a> {
    key: &'a str,
    kind: &'a str,
    file: &'a str,
    covers: &'a str,
    after: &'a str,
}

impl Task<'_> {
    fn yaml(&self) -> String {
        let mut text = format!(
            "  - key: {key}\n    title: Write {file}\n    ticket: tickets/{key}.md\n\
             \x20   kind: {kind}\n    profile: opus\n    covers: [{covers}]\n\
             \x20   outputs: [{file}]\n    acceptance:\n      - run: \"true\"\n",
            key = self.key,
            file = self.file,
            kind = self.kind,
            covers = self.covers,
        );
        if !self.after.is_empty() {
            text.push_str(&format!("    after: [{}]\n", self.after));
        }
        text
    }
}

/// A fleet with a test fleetd and 1 idea of `horch idea add`.
struct World {
    h: Harness,
    idea: String,
}

impl World {
    /// The fleet, with `scenario` as the keys of each fake agent, and its
    /// 1 idea.
    fn new(name: &str, scenario: &str) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger is in the state folder and the fleet root is next to
        // it, with no FLEET_ROOT: a pane gets only HORCH_STATE_DIR and
        // HORCH_DATA_DIR, and its process finds the same paths there.
        let fleet = h.state.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.state.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        // No test uses the network for the docs sync of fleetd.
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        // `exec`: each pane of the fake herdr runs its command.
        h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
        h.fleet_workspace();
        // Plan validation refuses a plan when the project declares no gates.
        std::fs::write(
            h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        let mut w = World {
            h,
            idea: String::new(),
        };
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);
        w.h.serve_fleetd();

        let o = w.h.run(&["idea", "add", IDEA]);
        assert!(o.status.success(), "horch idea add: {}", text(&o));
        // fleetd can add other ideas, for example 1 for each agent CLI of
        // the host that fails its conformance check: find the idea.md of
        // this idea.
        w.idea = w.wait("the idea row", || {
            w.rows("SELECT id FROM ideas ORDER BY id")
                .into_iter()
                .find(|id| {
                    let md =
                        w.h.state
                            .join("fleet")
                            .join("ideas")
                            .join(id)
                            .join("idea.md");
                    std::fs::read_to_string(md).is_ok_and(|t| t.contains(IDEA))
                })
        });
        w.wait_state("awaiting_approval");
        let o = w.h.run(&["idea", "approve", &w.idea]);
        assert!(o.status.success(), "horch idea approve: {}", text(&o));
        w.wait_state("ticketing");
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

    /// The idea folder.
    fn folder(&self) -> PathBuf {
        self.h.state.join("fleet").join("ideas").join(&self.idea)
    }

    /// Write `name` in the idea folder as the sub-orchestrator does: a
    /// temp file in the folder, then a rename, so fleetd never reads half
    /// a file.
    fn write(&self, name: &str, text: &str) {
        let path = self.folder().join(name);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        // SR-6 refuses a folder that is wider than 0700.
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(DIR_MODE)).unwrap();
        let tmp = self
            .folder()
            .join(format!(".{}.tmp", name.replace('/', "_")));
        std::fs::write(&tmp, text).unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(FILE_MODE)).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    }

    /// Write the ticket of each task, then plan.yaml with `tasks`.
    fn write_plan(&self, tasks: &[Task<'_>]) -> String {
        for t in tasks {
            self.write(
                &format!("tickets/{}.md", t.key),
                &ticket(t.key, t.kind, t.file),
            );
        }
        let feature = self.wait("the feature of the idea", || {
            self.one(&format!(
                "SELECT feature_id FROM ideas WHERE id = '{}' AND feature_id IS NOT NULL",
                self.idea
            ))
        });
        let mut plan = format!(
            "idea: {}\nrepo: .\nfeature: {feature}\nbase_branch: main\ntasks:\n",
            self.idea
        );
        for t in tasks {
            plan.push_str(&t.yaml());
        }
        self.write("plan.yaml", &plan);
        plan
    }

    /// The rows of `sql`, each row its columns joined with 1 space. No rows
    /// while the ledger does not exist yet.
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

    fn state(&self) -> Option<String> {
        self.one(&format!(
            "SELECT state FROM ideas WHERE id = '{}'",
            self.idea
        ))
    }

    fn wait_state(&self, state: &str) {
        self.wait(&format!("idea {} in {state}", self.idea), || {
            (self.state().as_deref() == Some(state)).then_some(())
        });
    }

    /// plan.verdict.json, when fleetd wrote it.
    fn verdict(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.folder().join("plan.verdict.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The `rule` of each problem of the refused plan.verdict.json.
    fn wait_refused(&self) -> (Value, Vec<String>) {
        let v = self.wait("a refused plan.verdict.json", || {
            self.verdict().filter(|v| v["valid"] == Value::Bool(false))
        });
        let rules = v["problems"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .filter_map(|p| p["rule"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        (v, rules)
    }

    /// The tasks of the idea, each "task_id work_kind".
    fn tasks(&self) -> Vec<String> {
        self.rows(&format!(
            "SELECT task_id, IFNULL(work_kind, '-') FROM tasks WHERE idea_id = '{}'
             ORDER BY task_id",
            self.idea
        ))
    }

    /// The refused plan wrote nothing of itself in the ledger, and the idea
    /// stays in `ticketing` with 1 more phase attempt.
    fn assert_nothing_committed(&self) {
        assert_eq!(
            self.state().as_deref(),
            Some("ticketing"),
            "{}",
            self.context()
        );
        assert!(self.tasks().is_empty(), "{}", self.context());
        let id = &self.idea;
        let reqs = self.rows(&format!(
            "SELECT req_id FROM requirements WHERE idea_id = '{id}'"
        ));
        assert!(reqs.is_empty(), "requirements: {reqs:?}");
        let covers = self.rows(&format!(
            "SELECT task_id FROM task_covers WHERE idea_id = '{id}'"
        ));
        assert!(covers.is_empty(), "task_covers: {covers:?}");
        let row = self.one(&format!(
            "SELECT IFNULL(plan_sha256, '-'), phase_attempts FROM ideas WHERE id = '{id}'"
        ));
        let row = row.unwrap_or_default();
        let (sha, tries) = row.split_once(' ').unwrap_or_default();
        assert_eq!(sha, "-", "plan_sha256 of a refused plan: {row}");
        assert!(
            tries.parse::<i64>().unwrap_or(0) >= 1,
            "phase_attempts: {row}"
        );
    }

    /// What a failed wait shows.
    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let stderr = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        let stderr: Vec<&str> = stderr.lines().filter(|l| seen.insert(*l)).collect();
        [
            show("ideas", "SELECT id, state, phase_attempts FROM ideas"),
            show(
                "features",
                "SELECT id, IFNULL(branch, '-'), state FROM features",
            ),
            show("needs-you items", "SELECT key, state, text FROM needs_you"),
            show(
                "attempts",
                "SELECT id, state, IFNULL(outcome, '-') FROM attempts",
            ),
            show(
                "tasks",
                "SELECT task_id, kind, status, IFNULL(idea_id, '-') FROM tasks",
            ),
            format!(
                "plan.verdict.json: {}\n",
                self.verdict().map(|v| v.to_string()).unwrap_or_default()
            ),
            format!("fleetd stderr, each line 1 time:\n{}\n", stderr.join("\n")),
        ]
        .concat()
    }

    /// Wait for `what`: `f` gives a value.
    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

/// FR-3: a plan with no task for the requirement R2 is refused. Nothing of
/// it reaches the ledger.
#[test]
fn fr3_plan_missing_a_requirement_is_rejected() {
    let w = World::new("p6-plans-fr3-missing", TO_TICKETING);
    w.write_plan(&[Task {
        key: "T1",
        kind: "feature",
        file: "fake.txt",
        covers: "R1",
        after: "",
    }]);
    let (v, rules) = w.wait_refused();
    assert!(rules.iter().any(|r| r == "coverage"), "{v}");
    assert!(v.to_string().contains("R2"), "the verdict names no R2: {v}");
    w.assert_nothing_committed();
}

/// FR-3: a valid plan of 2 tasks commits each task, its dependency, its
/// covers, the requirements and the plan hash, and the idea goes to
/// `in_progress`.
#[test]
fn fr3_valid_plan_commits_each_task() {
    let w = World::new("p6-plans-fr3-valid", TO_TICKETING);
    let plan = w.write_plan(&[
        Task {
            key: "T1",
            kind: "feature",
            file: "fake.txt",
            covers: "R1",
            after: "",
        },
        Task {
            key: "T2",
            kind: "feature",
            file: "fake2.txt",
            covers: "R2",
            after: "T1",
        },
    ]);
    let v = w.wait("a valid plan.verdict.json", || {
        w.verdict().filter(|v| v["valid"] == Value::Bool(true))
    });
    let id = &w.idea;
    let by_key = |key: &str| -> String {
        v["tasks"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| t["key"] == key))
            .and_then(|t| t["task"].as_str())
            .unwrap_or_else(|| panic!("the verdict has no task {key}: {v}"))
            .to_string()
    };
    let (t1, t2) = (by_key("T1"), by_key("T2"));
    let mut want = vec![format!("{t1} feature"), format!("{t2} feature")];
    want.sort();
    assert_eq!(w.tasks(), want, "{}", w.context());

    let deps = w.rows(&format!(
        "SELECT task_id, blocked_by FROM task_deps WHERE task_id IN ('{t1}', '{t2}')"
    ));
    assert_eq!(deps, vec![format!("{t2} {t1}")]);
    let mut covers = w.rows(&format!(
        "SELECT task_id, req_id FROM task_covers WHERE idea_id = '{id}'"
    ));
    covers.sort();
    let mut want = vec![format!("{t1} R1"), format!("{t2} R2")];
    want.sort();
    assert_eq!(covers, want);
    let reqs = w.rows(&format!(
        "SELECT req_id FROM requirements WHERE idea_id = '{id}' ORDER BY req_id"
    ));
    assert_eq!(reqs, vec!["R1".to_string(), "R2".to_string()]);

    let sha = sha256_bytes(plan.as_bytes()).hex();
    assert_eq!(v["plan_sha256"], Value::String(sha.clone()), "{v}");
    w.wait("the idea in in_progress with its plan hash", || {
        w.one(&format!(
            "SELECT 1 FROM ideas WHERE id = '{id}' AND state = 'in_progress'
               AND plan_sha256 = '{sha}'"
        ))
    });
}

/// FR-4: a task of a judgment kind with no judge task is refused, and
/// nothing of the plan reaches the ledger.
#[test]
fn fr4_judgment_task_without_judge_is_refused() {
    let w = World::new("p6-plans-fr4", TO_TICKETING);
    w.write_plan(&[Task {
        key: "T1",
        kind: "docs",
        file: "fake.md",
        covers: "R1, R2",
        after: "",
    }]);
    let (v, rules) = w.wait_refused();
    assert!(rules.iter().any(|r| r == "judges"), "{v}");
    w.assert_nothing_committed();
}

/// FM-10: the fake idea-lead writes a plan whose 2 tasks wait for each
/// other. fleetd refuses it with the `dag` rule, commits nothing, and a new
/// ticketing process starts after the refusal, with the errors in
/// plan.verdict.json and in `horch idea show`.
#[test]
fn fm10_cycle_is_refused_and_ticketing_reruns() {
    let w = World::new("p6-plans-fm10", "clarify,specify,ticket_cycle");
    let (v, rules) = w.wait_refused();
    assert!(rules.iter().any(|r| r == "dag"), "{v}");
    w.assert_nothing_committed();

    let id = &w.idea;
    let refused = w
        .one(&format!(
            "SELECT MAX(event_id) FROM events
             WHERE entity = 'idea' AND entity_id = '{id}' AND kind = 'verdict'"
        ))
        .filter(|e| e != "-")
        .unwrap_or_else(|| panic!("no verdict event of {id}: {}", w.context()));
    w.wait("a ticketing process that starts after the refusal", || {
        w.one(&format!(
            "SELECT entity_id FROM events
             WHERE entity = 'attempt' AND entity_id LIKE '{id}/ticketing/%'
             GROUP BY entity_id HAVING MIN(event_id) > {refused}"
        ))
    });

    let o = w.h.run(&["idea", "show", id]);
    assert!(o.status.success(), "horch idea show: {}", text(&o));
    let shown = String::from_utf8_lossy(&o.stdout);
    assert!(
        shown.contains("Plan verdict") && shown.contains("dag"),
        "horch idea show names no refused plan: {}",
        text(&o)
    );
}
