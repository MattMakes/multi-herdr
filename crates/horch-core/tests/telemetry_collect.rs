//! Collector ticks over the fixture corpus: every ledger, every agent, the
//! crash window, and the snapshot totals from `EXPECTED.md`.

mod common;

use std::collections::BTreeSet;

use common::*;
use horch_core::clock;
use horch_core::telemetry::collect::{Collector, Probing, Snapshot};
use horch_core::telemetry::store;
use horch_core::telemetry::Event;

fn now() -> chrono::DateTime<chrono::Utc> {
    clock::parse("2026-09-28T18:00:00Z").unwrap()
}

fn tick(w: &World) -> Snapshot {
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap()
}

fn events(w: &World) -> Vec<Event> {
    store::read_all(&horch_core::telemetry::dir(&w.state))
}

fn cost_of(events: &[Event], record: &str) -> f64 {
    events
        .iter()
        .filter(|e| e.record_id == record)
        .map(|e| e.cost_usd.unwrap_or(0.0))
        .sum()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn tel_01_every_agent_is_read() {
    let w = world(Part::Whole);
    let snap = tick(&w);
    let ev = events(&w);
    let agents: BTreeSet<&str> = ev.iter().map(|e| e.agent.as_str()).collect();
    let mut want: BTreeSet<&str> = ["claude", "codex", "pi", "prime"].into();
    if sqlite3().is_some() {
        want.insert("opencode");
    }
    assert_eq!(agents, want);
    let unread: Vec<(&str, &str)> = snap
        .unread
        .iter()
        .map(|u| (u.record_id.as_str(), u.reason.as_str()))
        .collect();
    assert!(
        unread.contains(&("rec-a6", "no session id yet")),
        "{unread:?}"
    );
    assert!(
        unread.contains(&("rec-b4", "agent none is not read")),
        "{unread:?}"
    );
    if sqlite3().is_none() {
        assert!(
            unread.contains(&("rec-b3", "sqlite3 not found")),
            "{unread:?}"
        );
    }
    // An unread record contributes no events, never a zero row.
    assert!(!ev
        .iter()
        .any(|e| e.record_id == "rec-a6" || e.record_id == "rec-b4"));
}

#[test]
fn tel_09_two_projects_both_counted() {
    let w = world(Part::Whole);
    tick(&w);
    let projects: BTreeSet<String> = events(&w)
        .iter()
        .filter_map(|e| e.project.clone())
        .collect();
    assert_eq!(
        projects,
        ["/work/alpha".to_string(), "/work/beta".to_string()].into()
    );
}

#[test]
fn tel_08_kind_and_phase_on_events() {
    let w = world(Part::Whole);
    tick(&w);
    let ev = events(&w);
    let o1 = ev.iter().find(|e| e.record_id == "rec-o1").unwrap();
    assert_eq!(
        (o1.kind.as_str(), o1.phase.as_deref()),
        ("orchestrator", Some("plan"))
    );
    let a2 = ev.iter().find(|e| e.record_id == "rec-a2").unwrap();
    assert_eq!(a2.kind, "worker");
    assert_eq!(a2.phase.as_deref(), Some("implementation"));
    // The fixture ledger predates the `plan` field; the task names the plan.
    assert_eq!(a2.plan.as_deref(), Some("golden-prompts-whitespace"));
    assert_eq!(a2.teammate, "sonnet");
    assert_eq!(a2.effort.as_deref(), Some("medium"));
}

#[test]
fn expected_md_whole_file_totals() {
    let w = world(Part::Whole);
    let snap = tick(&w);
    let ev = events(&w);
    for (record, cost) in [
        ("rec-o1", 0.067428),
        ("rec-a2", 0.0641806),
        ("rec-a3", 0.007680),
        ("rec-a4", 0.009800),
        ("rec-a5", 0.000744),
        ("rec-b1", 0.003550),
        ("rec-b2", 0.003120),
        ("rec-b3", 0.0),
    ] {
        assert!(
            close(cost_of(&ev, record), cost),
            "{record}: {}",
            cost_of(&ev, record)
        );
    }
    assert_eq!(ev.iter().filter(|e| e.record_id == "rec-o1").count(), 7);
    let total: f64 = ev.iter().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
    assert!(close(total, 0.1565026), "{total}");
    let seven: f64 = snap.rollups["7d"].by_kind.iter().map(|r| r.cost_usd).sum();
    assert!(close(seven, 0.1459586), "{seven}");
    let five: f64 = snap.rollups["5h"].by_agent.iter().map(|r| r.cost_usd).sum();
    assert!(close(five, 0.1347286), "{five}");
    let orch = snap.rollups["7d"]
        .by_kind
        .iter()
        .find(|r| r.key == "orchestrator")
        .unwrap();
    assert!(close(orch.cost_usd, 0.067428));
    // The orchestrator is working, so it is live; it sorts first.
    assert_eq!(snap.live[0].record_id, "rec-o1");
    assert!(snap.live.iter().any(|l| l.record_id == "rec-a6" && l.idle));
    assert_eq!(
        snap.insights.top_plan.as_deref(),
        Some("golden-prompts-whitespace")
    );
    let share = snap.insights.orchestrator_share.unwrap();
    assert!(close(share, 0.067428 / 0.1459586), "{share}");
    // A refusal line in the orchestrator's transcript reached the quota file.
    assert_eq!(
        snap.pools["claude"].refusal_seen_at.as_deref(),
        Some("2026-09-28T17:05:00Z")
    );
}

#[test]
fn expected_md_stage_one_then_stage_two() {
    let w = world(Part::FirstHalf);
    tick(&w);
    let total: f64 = events(&w).iter().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
    let opencode = 0.0;
    assert!(close(total, 0.0783506 + opencode), "stage 1: {total}");
    for (from, to) in targets(&w.home, &w.state) {
        copy(&from, &to, Part::Whole);
    }
    tick(&w);
    let ev = events(&w);
    let total: f64 = ev.iter().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
    assert!(close(total, 0.1565026), "stage 2: {total}");
    let keys: BTreeSet<_> = ev.iter().map(|e| e.key()).collect();
    assert_eq!(keys.len(), ev.len(), "no duplicate event key");
}

#[test]
fn tel_07_crash_between_append_and_cursor() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.fail_after_append = true;
    assert!(c.tick(now()).is_err(), "the injected fault stops the tick");
    drop(c);
    let dir = horch_core::telemetry::dir(&w.state);
    assert!(
        store::load_cursors(&dir).is_empty(),
        "cursors were never saved"
    );
    // Restart: everything is re-read from 0, and the repeats are dropped.
    tick(&w);
    tick(&w);
    let ev = events(&w);
    let lines: usize = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("events-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap().lines().count())
        .sum();
    assert_eq!(lines, ev.len(), "no duplicate line on disk");
    let total: f64 = ev.iter().map(|e| e.cost_usd.unwrap_or(0.0)).sum();
    assert!(close(total, 0.1565026), "{total}");
}

#[test]
fn a_ledger_mid_write_keeps_its_last_good_copy() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    let before = c.ledgers().len();
    std::fs::write(w.state.join("-work-alpha.json"), "[{\"record_id\": ").unwrap();
    assert_eq!(c.ledgers().len(), before);
}

#[test]
fn tel_11_no_content_or_identity_in_the_state_dir() {
    let w = world(Part::Whole);
    tick(&w);
    let dir = horch_core::telemetry::dir(&w.state);
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        for s in [
            "SENTINEL-CONTENT",
            "sentinel@example.invalid",
            "acct_",
            "user.email",
        ] {
            assert!(!text.contains(s), "{s} in {}", e.path().display());
        }
    }
}

/// A ledger with 1 Claude record for session `sid`, last updated at `updated`.
fn ledger_with(w: &World, name: &str, sid: &str, updated: &str) {
    let ledger = serde_json::json!([{
        "record_id": format!("rec-{name}"), "session_id": sid, "agent": "claude", "tier": "sonnet",
        "model": "claude-sonnet-5", "role": name, "status": "working", "task": "t", "history": [],
        "created_at": updated, "updated_at": updated
    }]);
    std::fs::write(w.state.join(format!("-{name}.json")), ledger.to_string()).unwrap();
}

/// A Claude assistant line with usage, from the A2 fixture, as message `id`.
fn assistant_line(w: &World, id: &str) -> String {
    let text = std::fs::read_to_string(
        w.home
            .join(format!(".claude/projects/-work-alpha/{A2}.jsonl")),
    )
    .unwrap();
    let line = text
        .lines()
        .find(|l| l.contains("\"assistant\"") && l.contains("\"usage\""))
        .unwrap();
    let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
    v["message"]["id"] = id.into();
    v.to_string() + "\n"
}

fn at(seconds: i64) -> chrono::DateTime<chrono::Utc> {
    now() + chrono::Duration::seconds(seconds)
}

/// NFR-02: a search for a transcript walks the transcript trees, so a
/// session with no transcript is not searched on every tick. A fresh record
/// (a new spawn) is searched every tick, and its transcript is read on the
/// tick it appears. An older one waits `SEARCH_AGAIN_S` between searches.
#[test]
fn nfr_02_a_record_with_no_transcript_is_not_searched_every_tick() {
    use horch_core::telemetry::collect::SEARCH_AGAIN_S;
    let w = world(Part::Whole);
    let fresh = "44444444-4444-4444-8444-444444444444";
    ledger_with(
        &w,
        "old",
        "55555555-5555-4555-8555-555555555555",
        "2026-09-28T17:00:00Z",
    );
    ledger_with(&w, "new", fresh, "2026-09-28T17:59:00Z");
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let first = c.searches;
    for s in [2, 4] {
        c.tick(at(s)).unwrap();
        assert_eq!(
            c.searches - first,
            (s / 2) as u64,
            "only the fresh record, at +{s} s"
        );
    }
    let before = c.searches;
    c.tick(at(SEARCH_AGAIN_S + 1)).unwrap();
    let again = c.searches - before;
    assert!(again >= 2, "the fresh one and the old one: {again}");
    c.tick(at(SEARCH_AGAIN_S + 3)).unwrap();
    assert_eq!(c.searches - before, again + 1, "the old one waits again");

    // The fresh record's transcript appears: read on that tick.
    let dir = w.home.join(".claude/projects/-work-new");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{fresh}.jsonl")),
        assistant_line(&w, "msg_w15_new"),
    )
    .unwrap();
    let snap = c.tick(at(SEARCH_AGAIN_S + 5)).unwrap();
    assert!(events(&w).iter().any(|e| e.record_id == "rec-new"));
    assert!(!snap.unread.iter().any(|u| u.record_id == "rec-new"));
}

/// A found transcript is kept across ticks while it is a file. When it
/// moves, the collector searches again, reads it at its new place, and the
/// store drops what it already holds.
#[test]
fn a_cached_transcript_that_moves_is_found_again() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let stored = events(&w).len();
    let searches = c.searches;
    c.tick(at(2)).unwrap();
    assert_eq!(
        c.searches, searches,
        "a found transcript is not searched again"
    );

    let line = assistant_line(&w, "msg_w15_moved");
    let moved = w.home.join(".claude/projects/-work-moved");
    std::fs::create_dir_all(&moved).unwrap();
    let to = moved.join(format!("{A2}.jsonl"));
    std::fs::rename(
        w.home
            .join(format!(".claude/projects/-work-alpha/{A2}.jsonl")),
        &to,
    )
    .unwrap();
    c.tick(at(4)).unwrap();
    assert_eq!(
        c.searches,
        searches + 1,
        "the moved transcript is searched for"
    );
    assert_eq!(events(&w).len(), stored, "no event twice");

    let mut f = std::fs::OpenOptions::new().append(true).open(&to).unwrap();
    std::io::Write::write_all(&mut f, line.as_bytes()).unwrap();
    c.tick(at(6)).unwrap();
    let ev = events(&w);
    assert_eq!(ev.len(), stored + 1);
    assert!(ev
        .iter()
        .any(|e| e.event_id == "msg_w15_moved" && e.record_id == "rec-a2"));
}

/// NFR-02: a tick with no new input rewrites no cursor file (it held 1,534
/// cursors, 2 MB, on the operator's Mac).
#[cfg(unix)]
#[test]
fn a_quiet_tick_does_not_rewrite_the_cursors() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let path = horch_core::telemetry::dir(&w.state).join("cursors.json");
    let ino = |p: &std::path::Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().ino()
    };
    let before = ino(&path);
    c.tick(at(2)).unwrap();
    assert_eq!(ino(&path), before, "a quiet tick keeps the cursor file");
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(
            w.home
                .join(format!(".claude/projects/-work-alpha/{A2}.jsonl")),
        )
        .unwrap();
    std::io::Write::write_all(&mut f, assistant_line(&w, "msg_w15_more").as_bytes()).unwrap();
    c.tick(at(4)).unwrap();
    assert_ne!(
        ino(&path),
        before,
        "new input saves the cursors (temp file + rename)"
    );
}
