#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::task_execution::application::dispatch::{choices, CheckSandboxes};
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Holds};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::{
    WorkerSandboxes, NO_CHECK_SANDBOX_KIND,
};
use horch_core::fleetd::worker_profiles::adapters::sandbox_rules::{
    refusal, refusal_of, NO_SANDBOX,
};
use horch_core::fleetd::worker_profiles::domain::operator_boundary::{Act, Boundary};
use horch_core::harness::claude::operator_boundary_on;
use horch_core::harness::codex::{self, CodexFacts, OPERATOR_BOUNDARY_SINCE};
use horch_core::harness::HarnessKind;
use horch_core::roster::{Roster, Teammate};
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, status};
use rusqlite::{Connection, TransactionBehavior};

const BOXED: &str = "claude-boxed";
const PI: &str = "pi-dev";

fn claude(name: &str, sandbox: Option<serde_json::Value>) -> Teammate {
    Teammate {
        name: name.into(),
        model: Some("opus".into()),
        sandbox: sandbox.map(|v| v.as_object().unwrap().clone()),
        ..Teammate::default()
    }
}

fn pi(fallback: Option<&str>) -> Teammate {
    Teammate {
        name: PI.into(),
        agent: HarnessKind::Pi,
        fallbacks: fallback.into_iter().map(str::to_string).collect(),
        ..Teammate::default()
    }
}

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    fleet_root: PathBuf,
    run: Run,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let top = horch_platform::path::canonicalize(dir.path()).unwrap();
    let fleet_root = top.join("fleet");
    let project = top.join("repo");
    std::fs::create_dir_all(&fleet_root).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let run = runs::create(
        &conn,
        &project,
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    World {
        _dir: dir,
        conn,
        fleet_root,
        run,
    }
}

impl World {
    fn sandboxes(&self, roster: &Roster) -> WorkerSandboxes {
        let codex = CodexSandbox {
            bin: PathBuf::from("/no/codex"),
            state_root: self.fleet_root.join("state"),
            personas: Vec::new(),
        };
        WorkerSandboxes {
            gap: None,
            ..WorkerSandboxes::on_path(codex, self.fleet_root.clone(), std::env::var_os("PATH"))
        }
        .with_profiles(roster)
    }

    fn ready(&self, title: &str, persona: &str) -> String {
        let new = NewTask::work(title, &format!("{title}.md"), persona);
        tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn pass(&mut self, sandboxes: &WorkerSandboxes) -> Vec<(String, String)> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let mut held = Vec::new();
        let mut bound = Vec::new();
        for choice in choices(&tx, &Holds::default(), sandboxes, None).unwrap() {
            match choice {
                Choice::Held { task_id, hold } => held.push((task_id, hold)),
                Choice::Bind { task_id, persona } => bound.push((task_id, persona)),
            }
        }
        sandboxes.move_to_fallbacks(&tx, &mut held).unwrap();
        sandboxes.open_items(&tx, &NeedsYouList, &held).unwrap();
        tx.commit().unwrap();
        bound
    }
}

fn roster(teammates: Vec<Teammate>) -> Roster {
    let mut r = Roster::default();
    for t in teammates {
        r.insert_for_test(t);
    }
    r
}

#[test]
fn profile_whose_sandbox_allows_the_operator_path_is_refused() {
    let all = Boundary {
        read_token_denied: true,
        connect_channel_denied: true,
        write_requests_denied: true,
    };
    assert_eq!(refusal_of(&Ok(all)), None, "a boundary that holds passes");
    for act in Act::ALL {
        let mut open = all;
        match act {
            Act::ReadToken => open.read_token_denied = false,
            Act::ConnectChannel => open.connect_channel_denied = false,
            Act::WriteRequests => open.write_requests_denied = false,
        }
        let why = refusal_of(&Ok(open)).unwrap_or_else(|| panic!("{act}: not refused"));
        assert!(why.contains(&act.to_string()), "{act}: {why}");
        for other in Act::ALL.into_iter().filter(|a| *a != act) {
            assert!(!why.contains(&other.to_string()), "{act}: {why}");
        }
    }
}

#[test]
fn profile_on_an_os_without_its_sandbox_is_refused() {
    let t = claude(BOXED, Some(serde_json::json!({})));
    let fleet_root = Path::new("/fleet");
    let why = refusal_of(&operator_boundary_on("windows", &t, fleet_root)).unwrap();
    assert!(why.starts_with(NO_SANDBOX), "{why}");
    assert!(why.contains("windows"), "the reason names the OS: {why}");
    for os in ["macos", "linux"] {
        assert_eq!(
            refusal_of(&operator_boundary_on(os, &t, fleet_root)),
            None,
            "{os}"
        );
    }
    let why = refusal(&pi(None), fleet_root).unwrap();
    assert!(why.starts_with(NO_SANDBOX), "{why}");
    assert!(why.contains("pi"), "{why}");
}

#[test]
fn worker_profile_with_no_sandbox_is_refused() {
    let fleet_root = Path::new("/fleet");
    let why = refusal(&claude("claude-open", None), fleet_root).unwrap();
    assert!(why.starts_with(NO_SANDBOX), "{why}");
    for act in Act::ALL {
        assert!(why.contains(&act.to_string()), "{act}: {why}");
    }
    assert_eq!(
        refusal(&claude(BOXED, Some(serde_json::json!({}))), fleet_root),
        None
    );
}

#[test]
fn old_orchestrator_seat_is_refused_like_each_worker_profile() {
    let fleet_root = Path::new("/fleet");
    for seat in ["orchestrator", "orchestrator-codex"] {
        assert!(refusal(&claude(seat, None), fleet_root).is_some(), "{seat}");
    }
    let r = roster(vec![
        claude("orchestrator", None),
        claude("claude-open", None),
    ]);
    let got = refused(&r, fleet_root);
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        ["claude-open", "orchestrator"]
    );
}

#[test]
fn refused_persona_gets_no_dispatch_and_1_needs_you_item() {
    let mut w = world();
    let r = roster(vec![pi(None), claude(BOXED, Some(serde_json::json!({})))]);
    let sandboxes = w.sandboxes(&r);
    assert!(!sandboxes.supported(PI));
    assert!(sandboxes.supported(BOXED));
    w.ready("a", PI);
    w.ready("b", PI);
    let c = w.ready("c", BOXED);
    for _ in 0..2 {
        let bound = w.pass(&sandboxes);
        assert_eq!(
            bound,
            [(c.clone(), BOXED.to_string())],
            "no task of {PI} binds"
        );
    }
    let items: Vec<_> = open_items(&w.conn)
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == NO_CHECK_SANDBOX_KIND)
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].key, format!("{NO_CHECK_SANDBOX_KIND}:{PI}"));
    assert!(items[0].text.contains(NO_SANDBOX), "{}", items[0].text);
    assert!(
        items[0].text.contains(&r#"the pi sandbox"#.to_string()),
        "{}",
        items[0].text
    );
}

#[test]
fn status_shows_not_available_on_this_os_no_sandbox() {
    let mut w = world();
    let r = roster(vec![pi(None)]);
    let sandboxes = w.sandboxes(&r);
    w.ready("a", PI);
    w.pass(&sandboxes);
    let report = status::report_for(&w.conn, &w.run, None).unwrap();
    let text = status::render(&report);
    let line = format!("Profile {PI}: {NO_SANDBOX}.");
    assert!(text.lines().any(|l| l == line), "{text}");
}

#[test]
fn fallback_with_a_sandbox_takes_the_task() {
    let mut w = world();
    let r = roster(vec![
        pi(Some(BOXED)),
        claude(BOXED, Some(serde_json::json!({}))),
    ]);
    let sandboxes = w.sandboxes(&r);
    let a = w.ready("a", PI);
    assert!(
        w.pass(&sandboxes).is_empty(),
        "the first pass moves the task"
    );
    let bound = w.pass(&sandboxes);
    assert_eq!(
        bound,
        [(a.clone(), BOXED.to_string())],
        "the next pass binds it"
    );
    let task = tasks::get(&w.conn, &a).unwrap();
    assert_eq!(task.claimable_by.as_deref(), Some(BOXED));
    assert!(
        open_items(&w.conn).unwrap().is_empty(),
        "a task that its fallback takes opens no item"
    );

    let r = roster(vec![pi(Some("claude-open")), claude("claude-open", None)]);
    let mut w = world();
    let sandboxes = w.sandboxes(&r);
    let a = w.ready("a", PI);
    for _ in 0..2 {
        assert!(w.pass(&sandboxes).is_empty());
    }
    let task = tasks::get(&w.conn, &a).unwrap();
    assert_eq!(task.claimable_by.as_deref(), Some(PI));
}

fn refused(roster: &Roster, fleet_root: &Path) -> BTreeMap<String, String> {
    let pinned_codex = CodexFacts {
        version: Some(format!("codex-cli {OPERATOR_BOUNDARY_SINCE}")),
        network_proxy: true,
    };
    roster
        .names()
        .into_iter()
        .filter_map(|name| {
            let t = roster.get(name)?;
            let why = match t.agent {
                HarnessKind::Codex => refusal_of(&codex::operator_boundary_on(
                    std::env::consts::OS,
                    &pinned_codex,
                    t,
                    fleet_root,
                )),
                _ => refusal(t, fleet_root),
            };
            why.map(|why| (name.to_string(), why))
        })
        .collect()
}

#[test]
fn builtin_roster_has_no_refused_worker_profile_on_macos() {
    let not_supported = |adapter| ("not supported", adapter);
    let recipe_pane = ("does not deny the operator acts", "");
    let expected: BTreeMap<&str, (&str, &str)> = BTreeMap::from([
        ("antigravity", not_supported("antigravity")),
        ("audio-reviewer", not_supported("antigravity")),
        ("spec-writer", not_supported("antigravity")),
        ("ticket-writer", not_supported("antigravity")),
        ("opencode-lightning", not_supported("opencode")),
        ("opencode-pickle", not_supported("opencode")),
        ("opencode-ultra", not_supported("opencode")),
        ("pi", not_supported("pi")),
        ("prime", not_supported("prime")),
        ("orchestration-orchestrator", recipe_pane),
        ("orchestration-worker", recipe_pane),
        ("orchestrator", recipe_pane),
        ("orchestrator-codex", recipe_pane),
    ]);
    let roster = Roster::builtin().unwrap();
    let fleet_root = Path::new("/fleet");
    let got = refused(&roster, fleet_root);
    let names: BTreeMap<&str, &str> = got
        .iter()
        .map(|(name, why)| (name.as_str(), why.as_str()))
        .collect();
    let unexpected: Vec<String> = names
        .iter()
        .filter(|(name, _)| !expected.contains_key(*name))
        .map(|(name, why)| format!("{name}: {why}"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "refused worker profiles:\n{}",
        unexpected.join("\n")
    );
    for (name, (cause, adapter)) in &expected {
        let why = names
            .get(name)
            .unwrap_or_else(|| panic!("{name} is no longer refused"));
        assert!(why.starts_with(NO_SANDBOX), "{name}: {why}");
        assert!(why.contains(cause), "{name}: {why}");
        assert!(why.contains(adapter), "{name}: {why}");
    }
}
