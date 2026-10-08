//! Collector ticks over the fixture corpus: every ledger, every agent, the
//! crash window, and the snapshot totals from `EXPECTED.md`.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use common::*;
use horch_core::clock;
use horch_core::telemetry::collect::{Collector, Probing, Snapshot, QUIET_RECENT_IDS};
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

/// NFR-02: a tick whose snapshot content is the same as the file's (all but
/// `generated_at`) does not rewrite `snapshot.json`. A tick with a new event
/// rewrites it. The file is compact JSON, and `Snapshot::read` reads it.
#[cfg(unix)]
#[test]
fn nfr_02_an_unchanged_snapshot_is_not_rewritten() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    let first = c.tick(now()).unwrap();
    let path = horch_core::telemetry::collect::snapshot_path(&w.state);
    let ino = |p: &std::path::Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().ino()
    };
    let before = ino(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("\n  "), "compact JSON");
    let read = Snapshot::read(&path).unwrap();
    assert_eq!(
        (read.generated_at, read.live.len()),
        (first.generated_at.clone(), first.live.len())
    );

    // 1 s earlier: no event crosses a window start (2 s later, one leaves 5h).
    let quiet = c.tick(at(-1)).unwrap();
    assert_eq!(ino(&path), before, "a quiet tick keeps the snapshot file");
    assert_eq!(c.timing.written("snapshot"), 0);
    assert_eq!(
        quiet.generated_at,
        clock::stamp(at(-1)),
        "the tick returns now"
    );
    let mut same = quiet.clone();
    same.generated_at = first.generated_at.clone();
    assert_eq!(same, first);

    // A new collector over the same files finds the content unchanged too.
    let mut again = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    again.info = c.info.clone();
    again.tick(at(-1)).unwrap();
    assert_eq!(
        ino(&path),
        before,
        "a restart over the same content keeps it"
    );

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(
            w.home
                .join(format!(".claude/projects/-work-alpha/{A2}.jsonl")),
        )
        .unwrap();
    std::io::Write::write_all(&mut f, assistant_line(&w, "msg_nfr02_snap").as_bytes()).unwrap();
    let grown = again.tick(at(-1)).unwrap();
    assert_ne!(ino(&path), before, "new content rewrites the snapshot");
    assert_eq!(
        Snapshot::read(&path).unwrap().generated_at,
        grown.generated_at
    );
}

/// NFR-02: a steady tick with no new event writes 0 bytes: no snapshot, no
/// cursors, no events and no `quota.json` (whose content, all but
/// `written_at`, did not change). A quota signal still writes `quota.json`.
#[cfg(unix)]
#[test]
fn nfr_02_a_quiet_tick_writes_nothing() {
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let quota = horch_core::telemetry::dir(&w.state).join("quota.json");
    assert!(quota.is_file(), "the first tick writes quota.json");
    let ino = |p: &std::path::Path| {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().ino()
    };
    let before = ino(&quota);
    c.tick(at(-1)).unwrap();
    assert_eq!(c.timing.written_total(), 0, "{}", c.timing.line());
    assert_eq!(ino(&quota), before, "a quiet tick keeps quota.json");

    // A Claude rate-limit refusal is a quota signal: quota.json changes.
    let refusal = serde_json::json!({
        "type": "assistant", "timestamp": "2026-09-28T17:59:30Z", "error": "rate_limit",
        "message": {"model": "<synthetic>", "id": "msg_nfr02_quota", "content": []}
    });
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(
            w.home
                .join(format!(".claude/projects/-work-alpha/{A2}.jsonl")),
        )
        .unwrap();
    std::io::Write::write_all(&mut f, format!("{refusal}\n").as_bytes()).unwrap();
    c.tick(at(-1)).unwrap();
    assert!(c.timing.written("quota") > 0, "{}", c.timing.line());
    assert_ne!(ino(&quota), before, "a refusal rewrites quota.json");
}

/// NFR-02: the snapshot is rebuilt on every tick; only its write is skipped
/// when nothing changed. When only time moves, a rollup window start moves
/// too: the events that leave the window leave its rollup, and the file is
/// rewritten with them gone.
#[test]
fn nfr_02_a_moving_window_drops_old_events_without_new_input() {
    use horch_core::telemetry::collect::{self, done_record_ids, read_ledgers};
    let w = world(Part::Whole);
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    let first = c.tick(now()).unwrap();
    let stored = events(&w).len();
    let later = at(3600);
    let moved = c.tick(later).unwrap();
    assert_eq!(c.timing.written("events"), 0, "no new input");
    assert_eq!(events(&w).len(), stored);

    let done = done_record_ids(&read_ledgers(&w.state));
    let since = collect::window_start("5h", later);
    let want = store::rollup(&events(&w), since.as_deref(), &done);
    assert_ne!(first.rollups["5h"], want, "an event leaves the 5h window");
    assert_eq!(moved.rollups["5h"], want);
    let on_disk = Snapshot::read(&collect::snapshot_path(&w.state)).unwrap();
    assert_eq!(on_disk.generated_at, clock::stamp(later), "rewritten");
    let total = |s: &Snapshot| -> u64 {
        s.rollups["5h"]
            .by_kind
            .iter()
            .map(|r| r.tokens.total())
            .sum()
    };
    assert_eq!(total(&on_disk), total(&moved));
    assert!(total(&moved) < total(&first));
}

/// A Claude session copied from the A2 fixture, with its own ledger.
/// Returns the transcript path.
fn copied_session(w: &World, name: &str, sid: &str, status: &str, updated: &str) -> PathBuf {
    let from = w
        .home
        .join(format!(".claude/projects/-work-alpha/{A2}.jsonl"));
    let to = w
        .home
        .join(format!(".claude/projects/-work-alpha/{sid}.jsonl"));
    std::fs::copy(from, &to).unwrap();
    session_ledger(w, name, sid, status, updated);
    to
}

fn session_ledger(w: &World, name: &str, sid: &str, status: &str, updated: &str) {
    let ledger = serde_json::json!([{
        "record_id": format!("rec-{name}"), "session_id": sid, "agent": "claude", "tier": "sonnet",
        "model": "claude-sonnet-5", "role": name, "status": status, "task": "t", "history": [],
        "created_at": "2026-09-01T00:00:00Z", "updated_at": updated
    }]);
    std::fs::write(w.state.join(format!("-{name}.json")), ledger.to_string()).unwrap();
}

/// Set the modification time of `path` to `when`.
fn age(path: &std::path::Path, when: chrono::DateTime<chrono::Utc>) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when.into())
        .unwrap();
}

fn cursor_json(w: &World, sid: &str) -> Option<serde_json::Value> {
    let path = horch_core::telemetry::dir(&w.state).join("cursors.json");
    let all: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (_, c) = all
        .as_object()
        .unwrap()
        .iter()
        .find(|(k, _)| k.starts_with(&format!("claude|{sid}|")))?;
    Some(c.clone())
}

fn session_events(w: &World, sid: &str) -> Vec<Event> {
    events(w)
        .into_iter()
        .filter(|e| e.session_id == sid)
        .collect()
}

/// NFR-02: the cursor of a done record whose transcript has not changed for
/// a day keeps only its newest message ids (the recent ids were 72% of a
/// 2.3 MB `cursors.json`). When the transcript grows again, the new message
/// is counted once and a repeated old line adds nothing.
#[test]
fn nfr_02_a_quiet_done_cursor_forgets_its_older_ids() {
    const SID: &str = "44444444-4444-4444-8444-444444444444";
    let w = world(Part::Whole);
    let path = copied_session(&w, "t2c", SID, "done", "2026-09-28T17:00:00Z");
    let more: Vec<String> = (0..6)
        .map(|i| assistant_line(&w, &format!("msg_old_{i}")))
        .collect();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut f, more.concat().as_bytes()).unwrap();
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let stored = session_events(&w, SID).len();
    assert!(stored > 0);
    let recent = |w: &World| -> Vec<String> {
        let cursor = cursor_json(w, SID).expect("kept: its record is in the window");
        (cursor["recent"].as_array().into_iter().flatten())
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert!(recent(&w).len() > QUIET_RECENT_IDS, "{:?}", recent(&w));

    age(&path, now() - chrono::Duration::hours(25));
    let later = at(60 * horch_core::telemetry::collect::TIDY_EVERY_MIN);
    c.tick(later).unwrap();
    let newest: Vec<String> = (2..6).map(|i| format!("msg_old_{i}")).collect();
    assert_eq!(recent(&w), newest);
    let cursor = cursor_json(&w, SID).unwrap();
    assert!(
        cursor["offset"].as_u64().unwrap() > 0,
        "still where it stopped"
    );

    // The last old line again, then 1 new message.
    let text = std::fs::read_to_string(&path).unwrap();
    let last = text
        .lines()
        .rfind(|l| l.contains("\"assistant\"") && l.contains("\"usage\""))
        .unwrap();
    let grown = format!("{last}\n{}", assistant_line(&w, "msg_nfr02_tidy"));
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut f, grown.as_bytes()).unwrap();
    c.tick(later + chrono::Duration::seconds(2)).unwrap();
    let ids: Vec<String> = session_events(&w, SID)
        .into_iter()
        .map(|e| e.event_id)
        .collect();
    assert_eq!(ids.len(), stored + 1, "{ids:?}");
    assert_eq!(ids.iter().filter(|i| *i == "msg_nfr02_tidy").count(), 1);
}

/// The review's scenario (R2): a done Claude session's transcript is quiet
/// for a day, the tidy runs, then Claude writes its last message again with
/// more tokens. The difference is stored as a correction, not lost as a
/// repeat of the stored id.
#[test]
fn r2_a_correction_after_recent_ids_are_forgotten_is_not_lost() {
    const SID: &str = "77777777-7777-4777-8777-777777777777";
    let w = world(Part::Whole);
    let path = copied_session(&w, "r2-correction", SID, "done", "2026-09-28T17:00:00Z");
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let total = |w: &World| -> u64 {
        session_events(w, SID)
            .iter()
            .map(|e| e.tokens.total())
            .sum()
    };
    let before = total(&w);

    age(&path, now() - chrono::Duration::hours(25));
    let later = at(60 * horch_core::telemetry::collect::TIDY_EVERY_MIN);
    c.tick(later).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let last = text
        .lines()
        .rfind(|l| l.contains("\"assistant\"") && l.contains("\"usage\""))
        .unwrap();
    let mut corrected: serde_json::Value = serde_json::from_str(last).unwrap();
    let output = corrected["message"]["usage"]["output_tokens"]
        .as_u64()
        .unwrap();
    assert!(output < 600);
    corrected["message"]["usage"]["output_tokens"] = 600.into();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut f, format!("{corrected}\n").as_bytes()).unwrap();
    c.tick(later + chrono::Duration::seconds(2)).unwrap();
    assert_eq!(total(&w), before + 600 - output, "the correction is stored");
}

/// NFR-02: the cursor of a session with no record in the retention window,
/// whose transcript has not changed for a day, is dropped; so is the cursor
/// of a file that is gone. When the session comes back and its transcript
/// grows, it is read again from the start, and the store drops every event
/// it already holds: only the new message is added.
#[test]
fn nfr_02_a_pruned_cursor_reads_again_from_the_start_without_double_counting() {
    const SID: &str = "55555555-5555-4555-8555-555555555555";
    const GONE: &str = "66666666-6666-4666-8666-666666666666";
    let w = world(Part::Whole);
    let path = copied_session(&w, "t2p", SID, "done", "2026-09-28T17:00:00Z");
    let gone = copied_session(&w, "t2g", GONE, "working", "2026-09-28T17:00:00Z");
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    let stored = session_events(&w, SID).len();
    assert!(stored > 0);
    assert!(cursor_json(&w, SID).is_some() && cursor_json(&w, GONE).is_some());

    // The record leaves the retention window; its transcript is quiet.
    session_ledger(&w, "t2p", SID, "done", "2026-08-01T00:00:00Z");
    age(&path, now() - chrono::Duration::days(2));
    std::fs::remove_file(&gone).unwrap();
    let later = at(60 * horch_core::telemetry::collect::TIDY_EVERY_MIN);
    c.tick(later).unwrap();
    assert!(cursor_json(&w, SID).is_none(), "pruned");
    assert!(cursor_json(&w, GONE).is_none(), "its file is gone");

    // It comes back with 1 new message.
    session_ledger(&w, "t2p", SID, "done", "2026-09-28T19:00:00Z");
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut f, assistant_line(&w, "msg_nfr02_back").as_bytes()).unwrap();
    c.tick(later + chrono::Duration::seconds(2)).unwrap();
    let cursor = cursor_json(&w, SID).expect("read again");
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(cursor["offset"].as_u64(), Some(len), "read to the end");
    let ids: Vec<String> = session_events(&w, SID)
        .into_iter()
        .map(|e| e.event_id)
        .collect();
    assert_eq!(ids.len(), stored + 1, "{ids:?}");
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "no event twice");
}

/// The review's scenario (R2): 40 days on, a restart deletes a session's
/// old event file with its keys, and the tidy drops its quiet cursor. When
/// the session comes back, its transcript is read from the start: the
/// events older than the retention window are dropped, not stored again.
#[test]
fn r2_a_pruned_cursor_does_not_restore_events_outside_retention() {
    const SID: &str = "88888888-8888-4888-8888-888888888888";
    let w = world(Part::Whole);
    let path = copied_session(&w, "r2-retention", SID, "done", "2026-09-28T17:00:00Z");
    let mut first = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    first.tick(now()).unwrap();
    assert!(session_events(&w, SID).len() > 1);
    drop(first);

    let far = now() + chrono::Duration::days(40);
    age(&path, far - chrono::Duration::hours(25));
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, far).unwrap();
    c.tick(far).unwrap();
    assert!(cursor_json(&w, SID).is_none(), "the old cursor is pruned");
    assert!(
        session_events(&w, SID).is_empty(),
        "the old event file is pruned"
    );

    session_ledger(&w, "r2-retention", SID, "working", &clock::stamp(far));
    let mut fresh: serde_json::Value =
        serde_json::from_str(&assistant_line(&w, "msg_r2_fresh")).unwrap();
    fresh["timestamp"] = clock::stamp(far).into();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut f, format!("{fresh}\n").as_bytes()).unwrap();
    c.tick(far + chrono::Duration::seconds(2)).unwrap();

    let stored = session_events(&w, SID);
    let ids: Vec<&str> = stored.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(ids, ["msg_r2_fresh"], "only the new event is stored");
    assert!(c.store.expired_dropped > 0);
}

/// R2 review: a live record keeps its quiet cursor although its
/// `updated_at` is older than the retention window.
#[test]
fn a_live_record_with_an_old_update_keeps_its_cursor() {
    const SID: &str = "99999999-9999-4999-8999-999999999999";
    let w = world(Part::Whole);
    let path = copied_session(&w, "live-old", SID, "working", "2026-09-28T17:00:00Z");
    let mut c = Collector::open_at(&w.state, w.loc.clone(), Probing::Never, now()).unwrap();
    c.tick(now()).unwrap();
    assert!(cursor_json(&w, SID).is_some());

    session_ledger(&w, "live-old", SID, "working", "2026-08-01T00:00:00Z");
    age(&path, now() - chrono::Duration::hours(25));
    c.tick(at(60 * horch_core::telemetry::collect::TIDY_EVERY_MIN))
        .unwrap();
    assert!(
        cursor_json(&w, SID).is_some(),
        "the live record's cursor is kept"
    );
}
