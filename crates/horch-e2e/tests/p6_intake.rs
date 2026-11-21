//! The e2e tests of intake and approval through the binary (ticket 56 of
//! fleetd-p6; `docs/specs/fleetd/design.md`: FR-1, FR-10, FR-12, FM-30,
//! FM-32).

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_core::digest::sha256_bytes;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const LATER_TICKS: i64 = 3;

const CODE_TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

const DOCS_TICKET: &str = "# 01: Write the queue page\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** docs\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `queue.md`\n\n\
## Goal\n\nThe page names the queue length.\n";

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
        h.fleet_workspace();
        let w = World { h };
        std::fs::write(
            w.h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);
        w.h.serve_fleetd();
        w
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.fleet_root()
    }

    fn folder(&self, idea: &str) -> PathBuf {
        self.fleet_root().join("ideas").join(idea)
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

    fn add(&self, idea: &str) -> String {
        let o = self.h.run(&["idea", "add", idea]);
        assert!(o.status.success(), "horch idea add: {}", text(&o));
        self.wait("the idea file of the new idea", || self.idea_with(idea))
    }

    fn idea_with(&self, needle: &str) -> Option<String> {
        self.rows("SELECT id FROM ideas ORDER BY id")
            .into_iter()
            .find(|id| {
                std::fs::read_to_string(self.folder(id).join("idea.md"))
                    .is_ok_and(|t| t.contains(needle))
            })
    }

    fn add_direct(&self, ticket: &str) -> String {
        let dir = self.h.root.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("01-ticket.md");
        std::fs::write(&path, ticket).unwrap();
        let o = self
            .h
            .run(&["idea", "add", "--direct", &path.to_string_lossy()]);
        assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
        let title = ticket.lines().next().unwrap();
        self.wait("the idea of the --direct ticket", || self.idea_with(title))
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

    fn idea_state(&self, idea: &str) -> Option<String> {
        self.one(&format!("SELECT state FROM ideas WHERE id = '{idea}'"))
    }

    fn idea_moves(&self, idea: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT to_state FROM events WHERE entity = 'idea' AND entity_id = '{idea}'
               AND to_state IS NOT NULL ORDER BY rowid"
        ))
    }

    fn tasks_of(&self, idea: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT kind, status FROM tasks WHERE idea_id = '{idea}' ORDER BY rowid"
        ))
    }

    fn fleet_clock(&self) -> Option<i64> {
        self.one("SELECT fleet_clock_s FROM fleetd WHERE id = 1")?
            .parse()
            .ok()
    }

    fn later_passes(&self) {
        let from = self.wait("the fleet clock", || self.fleet_clock());
        self.wait("later ticks of the fleet clock", || {
            (self.fleet_clock()? >= from + LATER_TICKS).then_some(())
        });
    }

    fn fake_extra(&self, key: &str) -> Value {
        self.wait(&format!("the fake idea-lead to log {key}"), || {
            self.h
                .calls_of("claude")
                .into_iter()
                .find_map(|c| c.get(key).cloned())
        })
    }

    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let mut calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .map(|c| format!("{} {}", c["fake"], c))
            .collect();
        let cut = calls.len().saturating_sub(30);
        calls.drain(..cut);
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        [
            show(
                "ideas",
                "SELECT id, state, IFNULL(approved_sha256, '-') FROM ideas",
            ),
            show(
                "idea events",
                "SELECT entity_id, kind, IFNULL(to_state, '-'), IFNULL(cause, '-') FROM events
                 WHERE entity = 'idea' ORDER BY rowid",
            ),
            show("needs-you items", "SELECT key, state, text FROM needs_you"),
            show(
                "tasks",
                "SELECT task_id, kind, status, IFNULL(idea_id, '-') FROM tasks",
            ),
            show(
                "requests",
                "SELECT request_id, kind, class, state, IFNULL(reason, '-') FROM requests",
            ),
            show("attempts", "SELECT * FROM attempts"),
            format!("calls:\n{}\n", calls.join("\n")),
            format!("idea files:\n{}\n", self.idea_files()),
            format!(
                "fleetd.stderr:\n{}\n",
                read(self.h.root.join("fleetd.stderr"))
            ),
        ]
        .concat()
    }

    fn idea_files(&self) -> String {
        let mut out = String::new();
        for id in self.rows("SELECT id FROM ideas ORDER BY id") {
            for name in ["idea.md", "plan.verdict.json"] {
                let path = self.folder(&id).join(name);
                if let Ok(t) = std::fs::read_to_string(&path) {
                    out.push_str(&format!("  {}:\n{t}\n", path.display()));
                }
            }
        }
        out
    }

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

#[test]
fn fr1_two_ideas_make_two_files_and_two_rows() {
    let w = World::new("p6-intake-fr1", "stay");
    let texts = [
        "Show the queue length in the status bar",
        "Log each refused request with its reason",
    ];
    for t in texts {
        let o = w.h.run(&["idea", "add", t]);
        assert!(o.status.success(), "horch idea add: {}", text(&o));
    }
    let ids: Vec<String> = texts
        .iter()
        .map(|t| w.wait("the idea file", || w.idea_with(t)))
        .collect();
    assert_ne!(ids[0], ids[1], "{}", w.context());
    for (id, t) in ids.iter().zip(texts) {
        let file = w.folder(id).join("idea.md");
        let body = std::fs::read_to_string(&file).unwrap();
        assert!(body.contains(t), "{}: {body}", file.display());
        let rows = w.rows(&format!("SELECT id FROM ideas WHERE id = '{id}'"));
        assert_eq!(rows.len(), 1, "{}", w.context());
        assert_eq!(
            w.idea_moves(id).first().map(String::as_str),
            Some("captured"),
            "{}",
            w.context()
        );
        w.later_passes();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), body);
    }
}

#[test]
fn fr10_sub_orchestrator_approval_leaves_awaiting_approval() {
    let w = World::new(
        "p6-intake-fr10",
        "clarify,await_approval,forge_approval,approve_inside",
    );
    let idea = w.add("Show the queue length in the status bar");
    let awaited = w.fake_extra("awaited");
    assert_eq!(awaited, "awaiting_approval", "{}", w.context());
    let approve = w.fake_extra("approve");
    assert_ne!(
        approve["exit"],
        0,
        "horch idea approve inside: {approve}\n{}",
        w.context()
    );
    let err = approve["stderr"].as_str().unwrap_or_default();
    assert!(err.contains("refused inside a fleet attempt"), "{approve}");
    assert!(
        w.folder(&idea).join("approval.json").is_file(),
        "the fake wrote approval.json: {}",
        w.context()
    );
    w.wait("the idea in awaiting_approval", || {
        (w.idea_state(&idea).as_deref() == Some("awaiting_approval")).then_some(())
    });
    w.later_passes();
    assert_eq!(
        w.idea_state(&idea).as_deref(),
        Some("awaiting_approval"),
        "{}",
        w.context()
    );
    let moves = w.idea_moves(&idea);
    assert!(!moves.iter().any(|s| s == "specifying"), "{moves:?}");
    assert_eq!(
        w.one(&format!(
            "SELECT IFNULL(approved_sha256, '-') FROM ideas WHERE id = '{idea}'"
        ))
        .as_deref(),
        Some("-"),
        "{}",
        w.context()
    );
    assert!(w.tasks_of(&idea).is_empty(), "{}", w.context());
}

fn valid_verdict(w: &World, idea: &str) -> Value {
    w.wait("a valid plan.verdict.json", || {
        let t = std::fs::read_to_string(w.folder(idea).join("plan.verdict.json")).ok()?;
        let v: Value = serde_json::from_str(&t).ok()?;
        (v["valid"] == true).then_some(v)
    })
}

#[test]
fn fr12_direct_idea_makes_1_validated_plan() {
    let w = World::new("p6-intake-fr12", "stay");
    let idea = w.add_direct(CODE_TICKET);
    let verdict = valid_verdict(&w, &idea);
    assert_eq!(
        verdict["tasks"].as_array().map(Vec::len),
        Some(1),
        "{verdict}"
    );
    let tasks = w.tasks_of(&idea);
    assert_eq!(tasks.len(), 1, "{tasks:?}\n{}", w.context());
    let plan = std::fs::read_to_string(w.folder(&idea).join("plan.yaml")).unwrap();
    assert!(
        plan.contains("run: 'true'")
            || plan.contains("run: \"true\"")
            || plan.contains("run: true"),
        "{plan}"
    );
    let moves = w.idea_moves(&idea);
    assert!(!moves.iter().any(|s| s == "clarifying"), "{moves:?}");
    assert_eq!(
        w.rows("SELECT task_id FROM tasks WHERE idea_id IS NULL"),
        Vec::<String>::new(),
        "each task belongs to an idea"
    );
}

#[test]
fn fr12_direct_judgment_idea_gets_its_judge() {
    let w = World::new("p6-intake-fr12-judge", "stay");
    let idea = w.add_direct(DOCS_TICKET);
    let verdict = valid_verdict(&w, &idea);
    assert_eq!(
        verdict["tasks"].as_array().map(Vec::len),
        Some(2),
        "{verdict}"
    );
    let tasks = w.tasks_of(&idea);
    assert_eq!(tasks.len(), 2, "{tasks:?}\n{}", w.context());
    let judges = w.rows(&format!(
        "SELECT j.task_id FROM tasks j JOIN tasks t ON t.task_id = j.parent_task_id
         WHERE j.idea_id = '{idea}' AND t.idea_id = '{idea}'"
    ));
    assert_eq!(judges.len(), 1, "{tasks:?}\n{}", w.context());
    assert_eq!(
        w.rows("SELECT task_id FROM tasks WHERE idea_id IS NULL"),
        Vec::<String>::new(),
        "each task belongs to an idea"
    );
}

fn assumption_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (n, rest) = l.split_once(". ")?;
            (rest.starts_with('[') && n.parse::<u32>().is_ok()).then(|| format!("A{n}"))
        })
        .collect()
}

#[test]
fn fm30_no_spec_before_approval() {
    let w = World::new("p6-intake-fm30", "clarify,specify,ticket");
    let idea = w.add("Show the queue length in the status bar");
    w.wait("the idea in awaiting_approval", || {
        (w.idea_state(&idea).as_deref() == Some("awaiting_approval")).then_some(())
    });
    w.wait("the spec of the fake", || {
        w.folder(&idea)
            .join("requirements.yaml")
            .is_file()
            .then_some(())
    });
    w.later_passes();
    assert_eq!(
        w.idea_state(&idea).as_deref(),
        Some("awaiting_approval"),
        "{}",
        w.context()
    );
    let reqs = format!("SELECT req_id FROM requirements WHERE idea_id = '{idea}'");
    assert!(w.rows(&reqs).is_empty(), "{}", w.context());
    assert!(
        !w.folder(&idea).join("plan.verdict.json").exists(),
        "{}",
        w.context()
    );
    assert!(w.tasks_of(&idea).is_empty(), "{}", w.context());

    let o = w.h.run(&["idea", "approve", &idea]);
    assert!(o.status.success(), "horch idea approve: {}", text(&o));
    w.wait("the spec of the approved idea to validate", || {
        w.idea_moves(&idea)
            .iter()
            .any(|m| m == "ticketing")
            .then_some(())
    });
    let moves = w.idea_moves(&idea);
    let at = |s: &str| moves.iter().position(|m| m == s);
    assert!(
        at("awaiting_approval") < at("specifying"),
        "specifying after approval: {moves:?}"
    );

    let folder = w.folder(&idea);
    let assumptions = std::fs::read(folder.join("assumptions.md")).unwrap();
    let approved = w
        .one(&format!(
            "SELECT approved_sha256 FROM ideas WHERE id = '{idea}'"
        ))
        .unwrap();
    assert_eq!(
        approved,
        sha256_bytes(&assumptions).hex(),
        "{}",
        w.context()
    );
    let ids = assumption_ids(&String::from_utf8_lossy(&assumptions));
    let yaml = std::fs::read_to_string(folder.join("requirements.yaml")).unwrap();
    let cites: Vec<&str> = yaml
        .lines()
        .filter_map(|l| l.trim().strip_prefix("cites:"))
        .collect();
    assert_eq!(cites.len(), 2, "1 cites line for each requirement: {yaml}");
    for line in cites {
        let named: Vec<&str> = line
            .trim()
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(str::trim)
            .collect();
        assert!(
            named.iter().any(|c| ids.iter().any(|id| id == c)),
            "{line} cites an approved assumption of {ids:?}"
        );
    }
}

#[test]
fn fm32_sub_orchestrator_operator_request_is_refused() {
    let w = World::new("p6-intake-fm32", "clarify,operator_request");
    let idea = w.add("Show the queue length in the status bar");
    let sent = w.fake_extra("operator_request");

    let channel = &sent["channel"];
    let reply: Value = channel["reply"]
        .as_str()
        .and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or(Value::Null);
    if reply["state"] == "refused" {
        let reason = reply["reason"].as_str().unwrap_or_default();
        assert!(reason.contains("operator token"), "{sent}");
    }
    assert!(
        channel["error"].is_string() || reply["state"] == "refused",
        "the channel request is refused: {sent}"
    );
    if let Some(path) = sent["file"]["path"].as_str() {
        let id = std::path::Path::new(path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let row = w.wait("the decided row of the request file", || {
            w.one(&format!(
                "SELECT state, IFNULL(reason, '-') FROM requests
                 WHERE request_id = '{id}' AND state != 'pending'"
            ))
        });
        assert!(row.starts_with("refused "), "{row}");
        assert!(row.contains("operator token"), "{row}");
    } else {
        assert!(sent["file"]["error"].is_string(), "{sent}");
    }

    w.wait("the idea in awaiting_approval", || {
        (w.idea_state(&idea).as_deref() == Some("awaiting_approval")).then_some(())
    });
    w.later_passes();
    assert_eq!(
        w.idea_state(&idea).as_deref(),
        Some("awaiting_approval"),
        "{}",
        w.context()
    );
    let applied =
        w.rows("SELECT request_id FROM requests WHERE kind = 'idea_approve' AND state = 'applied'");
    assert!(applied.is_empty(), "{applied:?}\n{}", w.context());
    let refused =
        w.rows("SELECT reason FROM requests WHERE kind = 'idea_approve' AND state = 'refused'");
    for reason in &refused {
        assert!(reason.contains("operator token"), "{refused:?}");
    }
}
