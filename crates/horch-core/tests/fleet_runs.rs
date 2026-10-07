//! Fleet run facts (`docs/specs/fleet-dataset.md` §3-5, FDS-01 to FDS-08,
//! FDS-22): the `fleet/` store, start rows, run rows, sync and verdicts.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use horch_core::competition::budget::UsageMeter;
use horch_core::competition::observe::TelemetryUsage;
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::records::Ledger;
use horch_core::fleet_runs::facts::{
    build_run_row, plan_file, record_run, record_start, RunSources, RunWrite, PLAN_TEXT_CAP,
};
use horch_core::fleet_runs::rows::{RunRow, StartRow, VerdictRow, WrittenBy};
use horch_core::fleet_runs::store::{self, read_rows, FleetFile};
use horch_core::fleet_runs::sync::sync_project;
use horch_core::fleet_runs::verdict::{record_verdict, Verdict, VerdictError};
use horch_core::measure::paths::DatasetPaths;
use horch_core::runtime::Inherited;
use horch_core::usage::Locations;
use horch_core::vcs::git::{GitCli, GitClient, GitIdentity};
use serde_json::{json, Value};

const NOW: &str = "2026-10-07T12:00:00Z";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

fn now() -> DateTime<Utc> {
    horch_core::clock::parse(NOW).unwrap()
}

/// A temp state root, home and project, with the sources a run row reads.
struct World {
    _tmp: tempfile::TempDir,
    state: PathBuf,
    home: PathBuf,
    project: PathBuf,
    paths: DatasetPaths,
    usage: TelemetryUsage,
    meter: UsageMeter,
    git: GitCli,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let home = tmp.path().join("home");
        let project = tmp.path().join("proj");
        for d in [&state, &home, &project] {
            std::fs::create_dir_all(d).unwrap();
        }
        let paths = DatasetPaths::new(&state, &project);
        World {
            usage: TelemetryUsage {
                locations: Locations::under_home(&home, &Inherited::default()),
            },
            meter: UsageMeter::default(),
            git: GitCli::new(PathBuf::from("git")),
            _tmp: tmp,
            state,
            home,
            project,
            paths,
        }
    }

    fn sources(&self) -> RunSources<'_> {
        RunSources {
            usage: &self.usage,
            meter: &self.meter,
            git: &self.git,
        }
    }

    fn ledger(&self) -> Ledger {
        Ledger::for_project(&self.state, &self.project.to_string_lossy())
    }

    /// Install the Claude usage fixture as the transcript of `session`.
    fn claude_transcript(&self, session: &str) -> PathBuf {
        let dir = self.home.join(".claude/projects/-proj");
        std::fs::create_dir_all(&dir).unwrap();
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usage/claude.jsonl");
        let to = dir.join(format!("{session}.jsonl"));
        std::fs::copy(from, &to).unwrap();
        to
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.project.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    fn runs(&self) -> Vec<RunRow> {
        read_rows::<RunRow>(&self.paths, FleetFile::Runs)
            .unwrap()
            .rows
    }
}

/// A finished worker record with every field the run row reads.
fn finished(id: &str, role: &str) -> Value {
    json!({
        "record_id": id,
        "session_id": SESSION,
        "agent": "claude",
        "tier": "backend-developer",
        "model": "claude-opus-5-5",
        "effort": "high",
        "phase": "implementation",
        "role": role,
        "status": "done",
        "task": "Read and follow ai_docs/plans/x.md exactly.",
        "history": [
            {"at": "2026-10-07T10:00:00Z", "event": "spawned", "text": "t"},
            {"at": "2026-10-07T10:10:00Z", "event": "note", "text": "step 1"},
            {"at": "2026-10-07T10:20:00Z", "event": "note", "text": "step 2"},
            {"at": "2026-10-07T10:30:00Z", "event": "done", "text": "first summary"},
            {"at": "2026-10-07T10:40:00Z", "event": "resumed", "text": "more"},
            {"at": "2026-10-07T11:00:00Z", "event": "done", "text": "last summary"}
        ],
        "created_at": "2026-10-07T10:00:00Z",
        "updated_at": "2026-10-07T11:00:05Z",
        "substitution_reason": "claude pool tight",
        "routing": {"requested": "backend-developer", "resolved": "backend-developer",
                    "fallback_index": null, "pool": "claude", "pool_state": "ok",
                    "reason": null, "mode": "auto"},
        "state": {"state": "done"},
        "exit_code": 0,
        "finished_at": "2026-10-07T11:00:00Z",
        "skills": [{"id": "tdd", "version": "bundled+abc", "source": "bundled",
                    "digest": "sha256:dcc5aa681b392557f13fe706d3be19ac74c6571f29e9dd508222972a74f4e073",
                    "policy": "deterministic"}],
        "project": "/work/proj"
    })
}

/// Append a record as given: `Ledger::insert` restamps it as a new spawn.
fn raw_insert(ledger: &Ledger, r: LedgerRecordV1) -> anyhow::Result<()> {
    ledger.store().insert(r)
}

fn rec(v: Value) -> LedgerRecordV1 {
    serde_json::from_value(v).unwrap()
}

/// FDS-01: `fleet/` is 0700, its files 0600, the lock lives in it, and 2
/// appends give 2 lines.
#[test]
fn fds_01_fleet_dir_modes_and_lock() {
    let w = World::new();
    assert_eq!(w.paths.fleet_dir(), w.paths.root().join("fleet"));
    store::append(&w.paths, FleetFile::Starts, &json!({"n": 1})).unwrap();
    {
        let _guard = store::lock(&w.paths).unwrap();
        assert!(w.paths.fleet_dir().join("fleet.lock").is_dir());
        store::append_locked(&w.paths, FleetFile::Starts, &json!({"n": 2})).unwrap();
    }
    assert!(!w.paths.fleet_dir().join("fleet.lock").exists());
    let text = std::fs::read_to_string(FleetFile::Starts.path(&w.paths)).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&w.paths.fleet_dir()), 0o700);
        assert_eq!(mode(w.paths.root()), 0o700);
        assert_eq!(mode(&FleetFile::Starts.path(&w.paths)), 0o600);
    }

    // A torn last line is skipped and counted; the next append ends it.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(FleetFile::Starts.path(&w.paths))
        .unwrap();
    f.write_all(b"{\"n\": 3").unwrap();
    let read = read_rows::<Value>(&w.paths, FleetFile::Starts).unwrap();
    assert_eq!((read.rows.len(), read.torn_lines), (2, 1));
    store::append(&w.paths, FleetFile::Starts, &json!({"n": 4})).unwrap();
    let read = read_rows::<Value>(&w.paths, FleetFile::Starts).unwrap();
    assert_eq!((read.rows.len(), read.torn_lines), (3, 1));
    assert_eq!(read.rows[2]["n"], 4);
}

/// FDS-02: the plan rule of §3.1 and the start row.
#[test]
fn fds_02_start_row_plan_rule() {
    let w = World::new();
    let p = &w.project;
    w.write("ai_docs/plans/a.md", "plan a");
    w.write("ai_docs/plans/b.md", "plan b");
    let abs = w._tmp.path().join("outside.md");
    std::fs::write(&abs, "outside").unwrap();

    // Trailing punctuation goes; the first existing file wins.
    let task = "Read and follow ai_docs/plans/a.md. Then ai_docs/plans/b.md";
    assert_eq!(
        plan_file(task, p),
        Some(PathBuf::from("ai_docs/plans/a.md"))
    );
    for tail in [",", ";", ":", ")", "\"", "'", ".)\""] {
        let task = format!("see ai_docs/plans/b.md{tail} now");
        assert_eq!(
            plan_file(&task, p),
            Some(PathBuf::from("ai_docs/plans/b.md")),
            "{task}"
        );
    }
    // A missing file is skipped; the next token that exists is taken.
    assert_eq!(
        plan_file("ai_docs/plans/none.md ai_docs/plans/b.md", p),
        Some(PathBuf::from("ai_docs/plans/b.md"))
    );
    assert_eq!(plan_file("ai_docs/plans/none.md only", p), None);
    assert_eq!(plan_file("no plan here", p), None);
    // A directory is not a plan file.
    std::fs::create_dir_all(p.join("dir.md")).unwrap();
    assert_eq!(plan_file("dir.md", p), None);
    // An absolute path inside the project becomes relative; outside it
    // stays absolute.
    let inside = p.join("ai_docs/plans/a.md");
    assert_eq!(
        plan_file(&format!("follow {}.", inside.display()), p),
        Some(PathBuf::from("ai_docs/plans/a.md"))
    );
    assert_eq!(
        plan_file(&format!("follow {}", abs.display()), p),
        Some(abs.clone())
    );

    // The start row: digest, text, no git outside a repository.
    let row = record_start(&w.paths, p, &w.git, "r-1", task, now()).unwrap();
    assert_eq!(row.schema, "mh.fleet-start/1.0.0");
    assert_eq!(row.record_id, "r-1");
    assert_eq!(row.at, NOW);
    assert_eq!(row.base_sha, None);
    assert_eq!(row.plan_path.as_deref(), Some("ai_docs/plans/a.md"));
    assert_eq!(
        row.plan_digest.as_deref(),
        Some(
            horch_core::measure::digest::sha256_bytes(b"plan a")
                .to_string()
                .as_str()
        )
    );
    assert!(row.plan_digest.as_deref().unwrap().starts_with("sha256:"));
    assert_eq!(row.plan_text.as_deref(), Some("plan a"));
    assert!(!row.plan_truncated);
    let rows = read_rows::<StartRow>(&w.paths, FleetFile::Starts).unwrap();
    assert_eq!(rows.rows, vec![row]);
    let raw = read_rows::<Value>(&w.paths, FleetFile::Starts)
        .unwrap()
        .rows;
    let keys: Vec<&str> = raw[0]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    let mut want = vec![
        "schema",
        "record_id",
        "at",
        "base_sha",
        "plan_path",
        "plan_digest",
        "plan_text",
        "plan_truncated",
    ];
    want.sort();
    assert_eq!(keys, want);

    // No plan: nulls.
    let none = record_start(&w.paths, p, &w.git, "r-2", "just do it", now()).unwrap();
    assert_eq!(
        (
            none.plan_path,
            none.plan_digest,
            none.plan_text,
            none.plan_truncated
        ),
        (None, None, None, false)
    );

    // The cap: 65 536 bytes, cut on a UTF-8 boundary.
    let big = format!("{}é{}", "a".repeat(PLAN_TEXT_CAP - 1), "b".repeat(100));
    w.write("big.md", &big);
    let row = record_start(&w.paths, p, &w.git, "r-3", "big.md", now()).unwrap();
    assert!(row.plan_truncated);
    let text = row.plan_text.unwrap();
    assert_eq!(text.len(), PLAN_TEXT_CAP - 1, "the 2-byte é does not fit");
    assert_eq!(
        row.plan_digest.unwrap(),
        horch_core::measure::digest::sha256_bytes(big.as_bytes()).to_string(),
        "the digest covers the whole file"
    );

    // In a git repository, base_sha is HEAD.
    let out = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(p)
        .output()
        .unwrap();
    assert!(out.status.success());
    let id = GitIdentity {
        name: "t".into(),
        email: "t@example.com".into(),
        date: NOW.into(),
    };
    let head = w.git.commit_all(p, "base", &id).unwrap().unwrap();
    let row = record_start(&w.paths, p, &w.git, "r-4", "x", now()).unwrap();
    assert_eq!(row.base_sha, Some(head));
}

/// FDS-03: every field of §3.2 from a fixture ledger record.
#[test]
fn fds_03_run_row_fields() {
    let w = World::new();
    let record = rec(finished("r-1", "backend-developer-1"));
    let start = StartRow {
        schema: "mh.fleet-start/1.0.0".into(),
        record_id: "r-1".into(),
        at: "2026-10-07T10:00:01Z".into(),
        base_sha: Some("b".repeat(40)),
        plan_path: Some("ai_docs/plans/x.md".into()),
        plan_digest: Some("sha256:00".into()),
        plan_text: Some("secret plan text".into()),
        plan_truncated: false,
    };
    let row = build_run_row(
        &record,
        Some(&start),
        &w.sources(),
        &w.project,
        now(),
        WrittenBy::Done,
    );
    let v = serde_json::to_value(&row).unwrap();
    let want = json!({
        "schema": "mh.fleet-run/1.0.0",
        "record_id": "r-1",
        "session_id": SESSION,
        "role": "backend-developer-1",
        "segment": 2,
        "kind": "worker",
        "project": "/work/proj",
        "project_slug": "-work-proj",
        "teammate": "backend-developer",
        "harness": "claude",
        "model": "claude-opus-5-5",
        "effort": "high",
        "phase": "implementation",
        "routing": {"requested": "backend-developer", "resolved": "backend-developer",
                    "fallback_index": null, "pool": "claude", "pool_state": "ok",
                    "reason": null, "mode": "auto"},
        "substitution_reason": "claude pool tight",
        "skills_expected": [{"id": "tdd", "version": "bundled+abc",
            "digest": "sha256:dcc5aa681b392557f13fe706d3be19ac74c6571f29e9dd508222972a74f4e073"}],
        "task": "Read and follow ai_docs/plans/x.md exactly.",
        "task_digest": horch_core::measure::digest::sha256_bytes(
            b"Read and follow ai_docs/plans/x.md exactly.").to_string(),
        "plan_path": "ai_docs/plans/x.md",
        "plan_digest": "sha256:00",
        "base_sha": "b".repeat(40),
        "started_at": "2026-10-07T10:40:00Z",
        "finished_at": "2026-10-07T11:00:00Z",
        "duration_ms": 1_200_000,
        "end": {"status": "done", "state": {"state": "done"}, "exit_code": 0},
        "done_summary": "last summary",
        "notes": 2,
        "resumed": true,
        "tokens": null,
        "cost_microusd": null,
        "cost_source": "unpriced",
        "transcript_ref": null,
        "transcript_digest": null,
        "usage_at": NOW,
        "head_sha": null,
        "written_by": "done",
        "horch_version": env!("CARGO_PKG_VERSION"),
    });
    assert_eq!(v, want);
    // The fields in the spec's order.
    let text = serde_json::to_string(&row).unwrap();
    let order = [
        "\"schema\"",
        "\"record_id\"",
        "\"session_id\"",
        "\"role\"",
        "\"segment\"",
        "\"kind\"",
        "\"project\"",
        "\"project_slug\"",
        "\"teammate\"",
        "\"harness\"",
        "\"model\"",
        "\"effort\"",
        "\"phase\"",
        "\"routing\"",
        "\"substitution_reason\"",
        "\"skills_expected\"",
        "\"task\"",
        "\"task_digest\"",
        "\"plan_path\"",
        "\"plan_digest\"",
        "\"base_sha\"",
        "\"started_at\"",
        "\"finished_at\"",
        "\"duration_ms\"",
        "\"end\"",
        "\"done_summary\"",
        "\"notes\"",
        "\"resumed\"",
        "\"tokens\"",
        "\"cost_microusd\"",
        "\"cost_source\"",
        "\"transcript_ref\"",
        "\"transcript_digest\"",
        "\"usage_at\"",
        "\"head_sha\"",
        "\"written_by\"",
        "\"horch_version\"",
    ];
    let at: Vec<usize> = order.iter().map(|k| text.find(k).unwrap()).collect();
    assert!(at.windows(2).all(|p| p[0] < p[1]), "{text}");
    assert!(
        !text.contains("secret plan text"),
        "no plan text in a run row"
    );

    // No finished_at: the last updated_at; no start row: nulls; an old
    // record without state, phase or routing.
    let mut v = finished("r-2", "r");
    let o = v.as_object_mut().unwrap();
    for k in [
        "finished_at",
        "state",
        "phase",
        "routing",
        "effort",
        "exit_code",
    ] {
        o.remove(k);
    }
    o.insert("history".into(), json!([]));
    let row = build_run_row(
        &rec(v),
        None,
        &w.sources(),
        &w.project,
        now(),
        WrittenBy::Sync,
    );
    assert_eq!(row.segment, 1);
    assert_eq!(row.started_at, "2026-10-07T10:00:00Z");
    assert_eq!(row.finished_at, "2026-10-07T11:00:05Z");
    assert_eq!(row.duration_ms, 3_605_000);
    assert_eq!(
        (row.plan_path, row.plan_digest, row.base_sha),
        (None, None, None)
    );
    assert_eq!((row.phase, row.routing, row.end.state), (None, None, None));
    assert_eq!((row.done_summary, row.notes, row.resumed), (None, 0, false));
    assert_eq!(row.written_by, WrittenBy::Sync);
}

/// FDS-03: a Claude transcript gives the tokens and a priced cost; the row
/// keeps its path and digest, not a copy.
#[test]
fn fds_03_run_row_tokens_from_transcript() {
    let w = World::new();
    let transcript = w.claude_transcript(SESSION);
    let record = rec(finished("r-1", "backend-developer-1"));
    let row = build_run_row(
        &record,
        None,
        &w.sources(),
        &w.project,
        now(),
        WrittenBy::Done,
    );
    let t = row.tokens.expect("tokens from the transcript");
    assert_eq!(
        (
            t.input,
            t.output,
            t.cache_read,
            t.cache_write_5m,
            t.cache_write_1h
        ),
        (15, 350, 50_000, 1000, 400)
    );
    let usage = w.usage.usage_for_test(&record);
    let (cost, source) = w.meter.price(&usage);
    assert!(cost.0 > 0);
    assert_eq!(row.cost_microusd, Some(cost.0));
    assert_eq!(row.cost_source, source.to_string());
    assert!(
        row.cost_source.starts_with("price_table@"),
        "{}",
        row.cost_source
    );
    assert_eq!(
        row.transcript_ref.as_deref(),
        Some(transcript.to_string_lossy().as_ref())
    );
    let bytes = std::fs::read(&transcript).unwrap();
    assert_eq!(
        row.transcript_digest,
        Some(horch_core::measure::digest::sha256_bytes(&bytes).to_string())
    );
    // Nothing under the dataset dir holds the transcript.
    assert!(
        !w.paths.root().exists()
            || walk(w.paths.root())
                .iter()
                .all(|p| { std::fs::read(p).map(|b| b != bytes).unwrap_or(true) })
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A test-only reader: the usage `TelemetryUsage` gives for a record.
trait UsageForTest {
    fn usage_for_test(&self, r: &LedgerRecordV1) -> horch_core::usage::Usage;
}

impl UsageForTest for TelemetryUsage {
    fn usage_for_test(&self, r: &LedgerRecordV1) -> horch_core::usage::Usage {
        horch_core::usage::read_session(&self.locations, &r.agent, r.session_id.as_deref())
            .unwrap()
            .1
    }
}

/// FDS-05: a second write for the same `record_id` writes nothing.
#[test]
fn fds_05_run_row_idempotent() {
    let w = World::new();
    let record = rec(finished("r-1", "a"));
    let first = record_run(
        &w.paths,
        &w.project,
        &record,
        &w.sources(),
        now(),
        WrittenBy::Done,
    )
    .unwrap();
    assert!(matches!(first, RunWrite::Written(_)), "{first:?}");
    let again = record_run(
        &w.paths,
        &w.project,
        &record,
        &w.sources(),
        now(),
        WrittenBy::Sync,
    )
    .unwrap();
    assert_eq!(again, RunWrite::AlreadyRecorded);
    // Sync finds it recorded too.
    let report = sync_project(
        &w.paths,
        &w.project,
        &[record],
        &w.sources(),
        now(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!((report.written, report.already), (0, 1));
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].written_by, WrittenBy::Done);

    // The start row of the record lands in its run row.
    let other = rec(finished("r-2", "b"));
    w.write("ai_docs/plans/x.md", "the plan");
    record_start(&w.paths, &w.project, &w.git, "r-2", &other.task, now()).unwrap();
    record_run(
        &w.paths,
        &w.project,
        &other,
        &w.sources(),
        now(),
        WrittenBy::Done,
    )
    .unwrap();
    assert_eq!(w.runs()[1].plan_path.as_deref(), Some("ai_docs/plans/x.md"));
}

/// FDS-05: a record that finishes, is resumed and finishes again gets a
/// second row: segment 2, started at the resume. A row written before
/// segments existed counts as segment 1.
#[test]
fn fds_05_resumed_record_new_segment() {
    let w = World::new();
    let mut v = finished("r-1", "a-1");
    let o = v.as_object_mut().unwrap();
    o.insert(
        "history".into(),
        json!([
            {"at": "2026-10-07T10:00:00Z", "event": "spawned", "text": "t"},
            {"at": "2026-10-07T10:30:00Z", "event": "done", "text": "first summary"}
        ]),
    );
    o.insert("finished_at".into(), json!("2026-10-07T10:30:00Z"));
    let first = rec(v.clone());
    let write = |r: &LedgerRecordV1, by| {
        record_run(&w.paths, &w.project, r, &w.sources(), now(), by).unwrap()
    };
    assert!(matches!(
        write(&first, WrittenBy::Done),
        RunWrite::Written(_)
    ));

    let o = v.as_object_mut().unwrap();
    let history = o["history"].as_array_mut().unwrap();
    history.push(json!({"at": "2026-10-07T10:40:00Z", "event": "resumed", "text": "more"}));
    history.push(json!({"at": "2026-10-07T11:00:00Z", "event": "done", "text": "last summary"}));
    o.insert("role".into(), json!("a-2"));
    o.insert("finished_at".into(), json!("2026-10-07T11:00:00Z"));
    let second = rec(v);
    assert!(matches!(
        write(&second, WrittenBy::Done),
        RunWrite::Written(_)
    ));
    assert_eq!(write(&second, WrittenBy::Sync), RunWrite::AlreadyRecorded);
    let report = sync_project(
        &w.paths,
        &w.project,
        &[second],
        &w.sources(),
        now(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!((report.written, report.already), (0, 1));

    let runs = w.runs();
    let got: Vec<_> = runs
        .iter()
        .map(|r| {
            (
                r.segment,
                r.role.as_str(),
                r.started_at.as_str(),
                r.duration_ms,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (1, "a-1", "2026-10-07T10:00:00Z", 1_800_000),
            (2, "a-2", "2026-10-07T10:40:00Z", 1_200_000),
        ]
    );

    // A row without `segment` reads as segment 1.
    let mut old = serde_json::to_value(&runs[0]).unwrap();
    old.as_object_mut().unwrap().remove("segment");
    old["record_id"] = json!("r-old");
    store::append(&w.paths, FleetFile::Runs, &old).unwrap();
    assert_eq!(w.runs()[2].segment, 1);
    let mut legacy = finished("r-old", "o");
    legacy["history"] = json!([]);
    assert_eq!(
        write(&rec(legacy), WrittenBy::Sync),
        RunWrite::AlreadyRecorded
    );
}

/// FDS-23: `horch done` stamps `finished_at` with the done time, also over
/// a stale one that a resume left.
#[test]
fn fds_23_done_stamps_finished_at() {
    let w = World::new();
    let ledger = w.ledger();
    let mut v = finished("r-1", "a-1");
    let o = v.as_object_mut().unwrap();
    o.insert("status".into(), json!("working"));
    o.insert("state".into(), json!({"state": "running"}));
    o.insert("finished_at".into(), json!("2026-10-07T10:30:00Z"));
    raw_insert(&ledger, rec(v)).unwrap();
    ledger.done("r-1", "all done").unwrap();
    let r = ledger.store().get("r-1").unwrap();
    let done_at = &r.history.last().unwrap().at;
    assert_eq!(r.history.last().unwrap().event, "done");
    assert_eq!(r.finished_at.as_deref(), Some(done_at.as_str()));
    assert_eq!(&r.updated_at, done_at);
    assert_ne!(done_at, "2026-10-07T10:30:00Z");
    assert_eq!(r.status, "done");
}

/// FDS-07: no transcript gives `tokens: null` and `cost_source: unpriced`.
#[test]
fn fds_07_missing_transcript_unpriced() {
    let w = World::new();
    let mut no_session = finished("r-2", "b");
    no_session["session_id"] = Value::Null;
    for record in [rec(finished("r-1", "a")), rec(no_session)] {
        record_run(
            &w.paths,
            &w.project,
            &record,
            &w.sources(),
            now(),
            WrittenBy::Done,
        )
        .unwrap();
    }
    for row in w.runs() {
        assert_eq!(row.tokens, None, "{}", row.record_id);
        assert_eq!(row.cost_microusd, None);
        assert_eq!(row.cost_source, "unpriced");
        assert_eq!((row.transcript_ref, row.transcript_digest), (None, None));
    }
}

/// FDS-06: sync writes a row for each finished record without one, and
/// none for a working record.
#[test]
fn fds_06_sync_finished_only() {
    let w = World::new();
    w.claude_transcript(SESSION);
    let mut working = finished("r-work", "w");
    working["status"] = json!("working");
    working["state"] = json!({"state": "running"});
    let mut planned = finished("r-plan", "p");
    planned["status"] = json!("working");
    planned["state"] = json!({"state": "planned"});
    let mut legacy_done = finished("r-old", "o");
    legacy_done.as_object_mut().unwrap().remove("state");
    let mut vanished = finished("r-gone", "g");
    vanished["state"] = json!({"state": "failed", "failure": {"kind": "pane_vanished"}});
    let mut launch = finished("r-launch", "l");
    launch["state"] = json!({"state": "launch_failed", "stage": "split", "reason": "abandoned"});
    let mut orch = finished("r-orch", "orchestrator");
    orch["kind"] = json!("orchestrator");
    orch["created_at"] = json!("2026-10-07T09:00:00Z");
    let records: Vec<LedgerRecordV1> = [
        finished("r-done", "d"),
        working,
        planned,
        legacy_done,
        vanished,
        launch,
        orch,
    ]
    .into_iter()
    .map(rec)
    .collect();
    // A sync writes at most `max_new` rows, oldest record first.
    let first = sync_project(&w.paths, &w.project, &records, &w.sources(), now(), 1).unwrap();
    assert_eq!((first.written, first.deferred), (1, 4), "{first:?}");
    assert_eq!(w.runs()[0].record_id, "r-orch");
    let report = sync_project(
        &w.paths,
        &w.project,
        &records,
        &w.sources(),
        now(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(report.written, 4, "{report:?}");
    assert_eq!((report.already, report.deferred), (1, 0));
    assert_eq!(report.with_tokens, 4);
    assert_eq!(report.unpriced, 0);
    assert_eq!(report.skipped.get("not finished"), Some(&2));
    let ids: Vec<String> = w.runs().into_iter().map(|r| r.record_id).collect();
    assert_eq!(ids, ["r-orch", "r-done", "r-old", "r-gone", "r-launch"]);
    assert!(w.runs().iter().all(|r| r.written_by == WrittenBy::Sync));
    assert_eq!(w.runs()[0].kind, "orchestrator");
    assert_eq!(
        w.runs()[3].end.state,
        Some(json!({"state": "failed", "failure": {"kind": "pane_vanished"}}))
    );

    // A second sync has nothing to do.
    let again = sync_project(
        &w.paths,
        &w.project,
        &records,
        &w.sources(),
        now(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!((again.written, again.already), (0, 5));
    assert_eq!(w.runs().len(), 5);
}

/// FDS-06: candidate and judge records get no row (§4.3). Their ledger
/// `kind` is `worker`; `experiment_id`, `round_id` and `label` mark them.
#[test]
fn fds_06_sync_skips_candidates_and_judges() {
    let w = World::new();
    let mut candidate = finished("r-cand", "candidate-A");
    candidate["experiment_id"] = json!("x");
    candidate["round_id"] = json!("r");
    candidate["label"] = json!("A");
    let mut judge = finished("r-judge", "judge");
    judge["round_id"] = json!("r");
    judge["label"] = json!("judge:1");
    let records: Vec<LedgerRecordV1> = [candidate, judge, finished("r-w", "w")]
        .into_iter()
        .map(rec)
        .collect();
    for r in &records[..2] {
        assert_eq!(r.kind, "worker", "the legacy kind does not tell them apart");
    }
    let report = sync_project(
        &w.paths,
        &w.project,
        &records,
        &w.sources(),
        now(),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(report.written, 1);
    assert_eq!(report.skipped.get("candidate or judge record"), Some(&2));
    assert_eq!(w.runs()[0].record_id, "r-w");
    // `horch done` of a candidate writes nothing either.
    let done = record_run(
        &w.paths,
        &w.project,
        &records[0],
        &w.sources(),
        now(),
        WrittenBy::Done,
    )
    .unwrap();
    assert_eq!(done, RunWrite::Skipped("candidate or judge record"));
    assert_eq!(w.runs().len(), 1);
}

/// FDS-08: a role names its newest record, a record id names itself; the
/// verdict row and the `verdict` history event are written.
#[test]
fn fds_08_verdict_resolves_and_appends() {
    let w = World::new();
    let ledger = w.ledger();
    let mut old = finished("r-old", "backend-developer-1");
    old["created_at"] = json!("2026-10-06T10:00:00Z");
    let mut working = finished("r-new", "backend-developer-1");
    working["status"] = json!("working");
    working["state"] = json!({"state": "running"});
    for v in [working, old, finished("r-other", "qa-1")] {
        raw_insert(&ledger, rec(v)).unwrap();
    }

    let row = record_verdict(
        &ledger,
        &w.paths,
        "backend-developer-1",
        "rework",
        Some("tests fail"),
        Some("orch-1"),
        now(),
    )
    .unwrap();
    assert_eq!(
        row,
        VerdictRow {
            schema: "mh.fleet-verdict/1.0.0".into(),
            record_id: "r-new".into(),
            role: "backend-developer-1".into(),
            verdict: "rework".into(),
            note: Some("tests fail".into()),
            at: NOW.into(),
            by: Some("orch-1".into()),
        },
        "the newest record with the role, in any state"
    );
    record_verdict(&ledger, &w.paths, "r-old", "accepted", None, None, now()).unwrap();

    let rows = read_rows::<VerdictRow>(&w.paths, FleetFile::Verdicts)
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[1].record_id.as_str(), rows[1].note.as_deref()),
        ("r-old", None)
    );
    let history = |id: &str| {
        let r = ledger.get(id).unwrap();
        let last = r.history.last().unwrap().clone();
        (last.event, last.text)
    };
    assert_eq!(
        history("r-new"),
        ("verdict".into(), "rework: tests fail".into())
    );
    assert_eq!(history("r-old"), ("verdict".into(), "accepted".into()));
    // `horch sessions` prints the verdict event.
    let text = horch_core::execution::store::ExecutionStore::render_text(&ledger.read().unwrap());
    assert!(text.contains("verdict @ "), "{text}");
    assert!(text.contains("rework: tests fail"), "{text}");

    assert_eq!("rejected".parse::<Verdict>().unwrap(), Verdict::Rejected);
    assert_eq!(Verdict::Accepted.to_string(), "accepted");
}

/// FDS-08: an unknown key, an orchestrator record and an unknown word are
/// refused, and nothing is written.
#[test]
fn fds_08_verdict_refusals_write_nothing() {
    let w = World::new();
    let ledger = w.ledger();
    let mut orch = finished("r-orch", "orchestrator");
    orch["kind"] = json!("orchestrator");
    raw_insert(&ledger, rec(orch)).unwrap();
    raw_insert(&ledger, rec(finished("r-w", "w-1"))).unwrap();
    let before = std::fs::read(ledger.path()).unwrap();

    let refuse = |key: &str, word: &str| {
        record_verdict(&ledger, &w.paths, key, word, Some("n"), None, now()).unwrap_err()
    };
    let e = refuse("nobody", "accepted");
    assert!(matches!(e, VerdictError::UnknownRecord(_)), "{e}");
    assert!(e.is_refusal());
    let e = refuse("orchestrator", "accepted");
    assert!(matches!(e, VerdictError::OrchestratorRecord(_)), "{e}");
    let e = refuse("r-orch", "rejected");
    assert!(matches!(e, VerdictError::OrchestratorRecord(_)), "{e}");
    let e = refuse("w-1", "great");
    assert!(matches!(e, VerdictError::UnknownVerdict(_)), "{e}");
    assert!(e.is_refusal());
    assert!(e.to_string().contains("accepted, rework or rejected"));

    assert_eq!(
        std::fs::read(ledger.path()).unwrap(),
        before,
        "the ledger is unchanged"
    );
    assert!(
        !FleetFile::Verdicts.path(&w.paths).exists(),
        "no verdict row"
    );
}

/// FDS-22 (FD5): the fleet store never changes a routing decision, and no
/// routing code names `fleet_runs`.
#[test]
fn fds_22_routing_ignores_fleet_store() {
    use horch_core::roster::Roster;
    use horch_core::routing::decision::{decide, GateFlags};
    use horch_core::routing::policy::{BalanceMode, Policy};
    use horch_core::routing::quota::{QuotaFile, QuotaView};

    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    let w = World::new();
    let mut roster = Roster::builtin().unwrap();
    roster.overlay(&core.join("../../teammates")).unwrap();
    let quota = horch_core::telemetry::dir(&w.state).join("quota.json");
    std::fs::create_dir_all(quota.parent().unwrap()).unwrap();
    std::fs::copy(
        core.join("tests/fixtures/telemetry/quota/claude-tight-codex-ok.json"),
        &quota,
    )
    .unwrap();
    let decisions = || {
        let view = QuotaView::new(
            QuotaFile::read(&quota).unwrap(),
            horch_core::clock::parse("2026-09-28T18:00:00Z").unwrap(),
            Policy::default(),
            true,
        );
        roster
            .names()
            .into_iter()
            .flat_map(|name| {
                let t = roster.require(name).unwrap();
                [BalanceMode::Off, BalanceMode::Auto].map(|mode| {
                    format!(
                        "{name}|{mode:?}: {:?}",
                        decide(t, &roster, &view, mode, GateFlags::default())
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    let without = decisions();

    // A full store: start, run and verdict rows for every teammate.
    let ledger = w.ledger();
    for (i, name) in roster.names().into_iter().enumerate() {
        let mut v = finished(&format!("r-{i}"), &format!("{name}-1"));
        v["tier"] = json!(name);
        let r = rec(v);
        raw_insert(&ledger, r.clone()).unwrap();
        record_start(&w.paths, &w.project, &w.git, &r.record_id, &r.task, now()).unwrap();
        record_run(
            &w.paths,
            &w.project,
            &r,
            &w.sources(),
            now(),
            WrittenBy::Done,
        )
        .unwrap();
        record_verdict(
            &ledger,
            &w.paths,
            &r.record_id,
            "rejected",
            None,
            None,
            now(),
        )
        .unwrap();
    }
    assert!(!w.runs().is_empty());
    assert_eq!(decisions(), without);

    // No routing source names the fleet store.
    let mut files = walk(&core.join("src/routing"));
    files.push(core.join("../horch/src/cmd/route.rs"));
    files.push(core.join("../horch/src/cmd/quotacmd.rs"));
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains("fleet_runs"),
            "{} names fleet_runs",
            f.display()
        );
    }
}

/// FDS-06 on real data: sync a copy of a real ledger. `HORCH_F1_LEDGER`
/// names the ledger copy, `HORCH_F1_STATE` the scratch state dir that gets
/// the fleet store. The transcripts are read from this machine's home.
#[test]
#[ignore = "reads a real ledger copy and this machine's transcripts"]
fn fds_06_live_sync_real_ledger() {
    let ledger = PathBuf::from(std::env::var("HORCH_F1_LEDGER").expect("HORCH_F1_LEDGER"));
    let state = PathBuf::from(std::env::var("HORCH_F1_STATE").expect("HORCH_F1_STATE"));
    let records: Vec<LedgerRecordV1> =
        serde_json::from_str(&std::fs::read_to_string(&ledger).unwrap()).unwrap();
    let project = records
        .iter()
        .find_map(|r| r.project.clone())
        .map(PathBuf::from)
        .expect("a record names its project");
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let usage = TelemetryUsage {
        locations: Locations::under_home(&home, &Inherited::default()),
    };
    let meter = UsageMeter::default();
    let git = GitCli::new(PathBuf::from("git"));
    let sources = RunSources {
        usage: &usage,
        meter: &meter,
        git: &git,
    };
    let paths = DatasetPaths::new(&state, &project);
    let started = std::time::Instant::now();
    let report =
        sync_project(&paths, &project, &records, &sources, Utc::now(), usize::MAX).unwrap();
    println!("records: {}", records.len());
    println!("sync took: {} ms", started.elapsed().as_millis());
    println!("{report:#?}");
    let started = std::time::Instant::now();
    let again = sync_project(&paths, &project, &records, &sources, Utc::now(), usize::MAX).unwrap();
    println!(
        "second sync took: {} ms, written {}",
        started.elapsed().as_millis(),
        again.written
    );
    assert_eq!(again.written, 0);
    let runs = read_rows::<RunRow>(&paths, FleetFile::Runs).unwrap();
    assert_eq!(runs.torn_lines, 0);
    let mut by_harness = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for r in &runs.rows {
        let e = by_harness
            .entry(r.harness.clone().unwrap_or_default())
            .or_default();
        e.0 += 1;
        e.1 += u32::from(r.tokens.is_some());
    }
    println!("rows by harness (rows, with tokens): {by_harness:?}");
}
