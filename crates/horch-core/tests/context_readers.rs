//! The current-context readers against the fixture home in
//! `tests/fixtures/context/home` (design §6.6, §9). Each expected number is
//! the sum of 1 usage line of the fixture, computed by hand.

use std::io::Write;
use std::path::{Path, PathBuf};

use horch_core::runtime::Inherited;
use horch_core::telemetry::context::{read_current, scan_tail, Reading};
use horch_core::telemetry::readers::Unreadable;
use horch_core::usage::Locations;
use serde_json::Value;

const PRE: &str = "11111111-1111-4111-8111-111111111111";
const PENDING: &str = "22222222-2222-4222-8222-222222222222";
const POST: &str = "33333333-3333-4333-8333-333333333333";
const ITERATIONS: &str = "44444444-4444-4444-8444-444444444444";
const CODEX: &str = "01a0de6e-0000-7000-8000-00000000000";
const PI: &str = "aaaaaaaa-0000-4000-8000-00000000000";

fn home() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context/home")
}

fn fixture_loc() -> Locations {
    Locations::under_home(&home(), &Inherited::default())
}

fn read(agent: &str, sid: &str) -> Reading {
    let cache = tempfile::tempdir().unwrap();
    read_current(&fixture_loc(), cache.path(), agent, Some(sid)).unwrap()
}

fn codex(n: u8) -> String {
    format!("{CODEX}{n}")
}

fn pi(n: u8) -> String {
    format!("{PI}{n}")
}

/// A Claude assistant line with 1 usage figure: `tokens` in `input_tokens`.
fn claude_usage_line(id: &str, tokens: u64) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"2026-09-20T12:00:00.000Z","message":{{"id":"{id}","model":"claude-opus-5-5","role":"assistant","usage":{{"input_tokens":{tokens},"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}}}}}"#
    )
}

/// A temp home with 1 Claude transcript; returns the home and the file.
fn claude_home(sid: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join(".claude/projects/-t");
    std::fs::create_dir_all(&projects).unwrap();
    let file = projects.join(format!("{sid}.jsonl"));
    (dir, file)
}

/// The 1 marker cache file under `cache_dir`, parsed.
fn marker_cache(cache_dir: &Path) -> Value {
    let files: Vec<_> = std::fs::read_dir(cache_dir.join("markers"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let name = files[0].file_name().unwrap().to_str().unwrap().to_string();
    assert_eq!(name.len(), "0123456789abcdef.json".len(), "{name}");
    serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap()
}

#[test]
fn ctx_02_claude_last_main_chain_response_with_output() {
    let r = read("claude", PRE);
    // msg_A: 4 + 2,929 + 308,000 + 292. The sidechain line (901,001) and
    // the <synthetic> line come after it and do not count.
    assert_eq!(r.tokens, Some(311_225));
    assert!(!r.pending);
    assert!(!r.provisional);
    assert_eq!(r.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(r.window, None);
    assert_eq!(r.last_compaction, None);
    assert_eq!(r.marks, 0);
    assert!(r.transcript.ends_with(format!("{PRE}.jsonl")));
}

#[test]
fn ctx_02_claude_last_message_iteration() {
    // The last `message` iteration: 2 + 1,918 + 229,078 + 79. The top-level
    // usage (460,258) and the advisor iteration do not count.
    assert_eq!(read("claude", ITERATIONS).tokens, Some(231_077));
}

#[test]
fn ctx_02_codex_last_token_count_total_and_window() {
    let r = read("codex", &codex(1));
    assert_eq!(r.tokens, Some(227_306));
    assert_eq!(r.window, Some(258_400));
    assert_eq!(r.model.as_deref(), Some("gpt-5.6-sol"));
    assert!(!r.pending);
    // The nested `{"type":"compacted"}` inside a token_count payload is not
    // a marker.
    assert_eq!(r.last_compaction, None);
    assert_eq!(r.marks, 0);
}

#[test]
fn ctx_03_claude_pending_uses_post_tokens_provisional() {
    let r = read("claude", PENDING);
    assert_eq!(r.tokens, Some(10_486));
    assert!(r.provisional);
    assert!(r.pending);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.at, "2026-09-20T10:05:00.000Z");
    assert_eq!(m.trigger.as_deref(), Some("manual"));
    assert_eq!(m.pre_tokens, Some(312_857));
    assert_eq!(m.post_tokens, Some(10_486));
    assert_eq!(r.marks, 1);
}

#[test]
fn ctx_03_claude_new_response_ends_pending() {
    let r = read("claude", POST);
    // msg_B: 3 + 30,000 + 3,000 + 349.
    assert_eq!(r.tokens, Some(33_352));
    assert!(!r.pending);
    assert!(!r.provisional);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.at, "2026-09-20T10:05:00.000Z");
    assert_eq!(r.marks, 1);
}

#[test]
fn ctx_03_codex_compacted_is_pending_until_token_count() {
    let r = read("codex", &codex(3));
    assert!(r.pending);
    assert_eq!(r.tokens, None);
    assert!(!r.provisional);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.at, "2026-09-20T11:00:00.000Z");
    assert_eq!(m.pre_tokens, Some(229_420));

    let r = read("codex", &codex(2));
    assert_eq!(r.tokens, Some(12_847));
    assert_eq!(r.window, Some(258_400));
    assert!(!r.pending);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.pre_tokens, Some(229_420));
    assert_eq!(m.trigger, None);
    assert_eq!(r.marks, 1);
}

#[test]
fn ctx_04_no_session_id_is_no_session() {
    let cache = tempfile::tempdir().unwrap();
    for agent in ["claude", "codex"] {
        let got = read_current(&fixture_loc(), cache.path(), agent, None);
        assert_eq!(got, Err(Unreadable::NoSessionId), "{agent}");
        let got = read_current(&fixture_loc(), cache.path(), agent, Some(""));
        assert_eq!(got, Err(Unreadable::NoSessionId), "{agent}");
    }
}

#[test]
fn ctx_04_missing_transcript_is_no_transcript() {
    let cache = tempfile::tempdir().unwrap();
    for agent in ["claude", "codex"] {
        let got = read_current(
            &fixture_loc(),
            cache.path(),
            agent,
            Some("99999999-9999-4999-8999-999999999999"),
        );
        assert!(
            matches!(got, Err(Unreadable::NoTranscript(_))),
            "{agent}: {got:?}"
        );
    }
}

#[test]
fn ctx_04_harness_without_reader_is_not_read() {
    let cache = tempfile::tempdir().unwrap();
    let got = read_current(&fixture_loc(), cache.path(), "antigravity", Some(PRE));
    assert_eq!(
        got,
        Err(Unreadable::NotRead("unknown agent 'antigravity'".into()))
    );
}

#[test]
fn ctx_04_opencode_without_sqlite3_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    std::fs::write(&db, b"").unwrap();
    let loc = Locations {
        opencode_db: db,
        sqlite3: dir.path().join("no-such-sqlite3"),
        ..fixture_loc()
    };
    let got = read_current(&loc, dir.path(), "opencode", Some("ses_pickle"));
    assert_eq!(got, Err(Unreadable::NotRead("sqlite3 not on PATH".into())));
}

#[test]
fn ctx_02_pi_skips_error_and_tool_result_lines() {
    // The `stop` line: totalTokens 201,000. The `error` line after it (zero
    // usage) and the toolResult line (999,999) do not count.
    let r = read("pi", &pi(1));
    assert_eq!(r.tokens, Some(201_000));
    assert_eq!(r.model.as_deref(), Some("qwen3.8"));
    assert!(!r.pending);
    assert!(!r.provisional);
    assert_eq!(r.window, None);
    assert_eq!(r.last_compaction, None);
    assert_eq!(r.marks, 0);
}

#[test]
fn ctx_03_pi_pending_is_unknown() {
    // A trailing `compaction` entry: pending, and pi writes no post figure.
    let r = read("pi", &pi(2));
    assert!(r.pending);
    assert_eq!(r.tokens, None);
    assert!(!r.provisional);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.at, "2026-09-20T12:00:00.000Z");
    assert_eq!(m.pre_tokens, Some(201_500));
    assert_eq!(m.post_tokens, None);
    assert_eq!(r.marks, 1);
}

#[test]
fn ctx_02_prime_after_compaction() {
    // Prime's record keeps the session file's path as its session id.
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/context/prime/2026-09-20-prime-post.jsonl");
    let r = read("prime", file.to_str().unwrap());
    assert_eq!(r.tokens, Some(25_000));
    assert!(!r.pending);
    assert_eq!(r.model.as_deref(), Some("claude-opus-5-5"));
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.pre_tokens, Some(280_000));
    assert_eq!(r.marks, 1);
    assert_eq!(r.transcript, file);
}

/// A temp `opencode.db` built from `opencode/rows.sql`, or None (with a
/// printed reason) when `sqlite3` is not on PATH. `HORCH_REQUIRE_SQLITE=1`
/// (the gate) turns the skip into a failure.
fn opencode_db() -> Option<(tempfile::TempDir, Locations)> {
    let sql = std::fs::File::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context/opencode/rows.sql"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    match std::process::Command::new("sqlite3")
        .arg(&db)
        .stdin(sql)
        .status()
    {
        Ok(s) => assert!(s.success(), "sqlite3 < rows.sql: {s}"),
        Err(e) => {
            assert!(
                std::env::var_os("HORCH_REQUIRE_SQLITE").is_none(),
                "HORCH_REQUIRE_SQLITE is set and sqlite3 does not run: {e}"
            );
            eprintln!("skipped: sqlite3 does not run: {e}");
            return None;
        }
    }
    let loc = Locations {
        opencode_db: db,
        ..fixture_loc()
    };
    Some((dir, loc))
}

#[test]
fn ctx_02_opencode_newest_finished_row() {
    let Some((dir, loc)) = opencode_db() else {
        return;
    };
    // The unfinished row after it does not count.
    let r = read_current(&loc, dir.path(), "opencode", Some("ses_pickle")).unwrap();
    assert_eq!(r.tokens, Some(17_009));
    assert_eq!(r.model.as_deref(), Some("opencode/big-pickle"));
    assert!(!r.pending);
    assert_eq!(r.last_compaction, None);
    assert_eq!(r.marks, 0);
    assert_eq!(r.transcript, loc.opencode_db);

    // After the summary row: `tokens.total` is 0, so the sum 1,500 + 500 +
    // 7,500. The summary row itself (31,200) is not a response.
    let r = read_current(&loc, dir.path(), "opencode", Some("ses_post")).unwrap();
    assert_eq!(r.tokens, Some(9_500));
    assert!(!r.pending);
    let m = r.last_compaction.expect("a compaction mark");
    assert_eq!(m.pre_tokens, Some(30_000));
    assert_eq!(r.marks, 1);
    // The database is queried; no marker cache is written.
    assert!(!dir.path().join("markers").exists());

    // An id that is not `ses_<alnum>` never reaches the SQL text.
    for bad in ["ses_x' OR 1=1 --", "abc", "ses_", "ses_a-b"] {
        let got = read_current(&loc, dir.path(), "opencode", Some(bad));
        assert!(matches!(got, Err(Unreadable::Failed(_))), "{bad}: {got:?}");
    }
}

#[cfg(unix)]
#[test]
fn ctx_10_cache_write_failure_keeps_the_reading() {
    use std::os::unix::fs::PermissionsExt;
    // The cache dir cannot be created (a read-only parent), as in a
    // sandboxed Codex worker.
    let parent = tempfile::tempdir().unwrap();
    let cache = parent.path().join("context");
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let got = read_current(&fixture_loc(), &cache, "claude", Some(PENDING));
    // The markers dir exists but is read-only.
    let cache2 = tempfile::tempdir().unwrap();
    let markers = cache2.path().join("markers");
    std::fs::create_dir(&markers).unwrap();
    std::fs::set_permissions(&markers, std::fs::Permissions::from_mode(0o500)).unwrap();
    let got2 = read_current(&fixture_loc(), cache2.path(), "codex", Some(&codex(2)));
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&markers, std::fs::Permissions::from_mode(0o700)).unwrap();

    let r = got.expect("the reading, not an error");
    assert_eq!(r.tokens, Some(10_486));
    assert!(r.pending);
    assert_eq!(r.marks, 1);
    assert!(!cache.exists());
    let r = got2.expect("the reading, not an error");
    assert_eq!(r.tokens, Some(12_847));
    assert_eq!(r.marks, 1);
    assert_eq!(std::fs::read_dir(&markers).unwrap().count(), 0);
}

#[test]
fn ctx_26_tail_read_is_bounded() {
    const SID: &str = "55555555-5555-4555-8555-555555555555";
    let big = format!(
        r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
        "x".repeat(5 << 20)
    );
    let (home, file) = claude_home(SID);
    let loc = Locations::under_home(home.path(), &Inherited::default());
    let cache = tempfile::tempdir().unwrap();

    // 1 usable line, a 5 MiB line, 1 more usable line: the last one wins.
    let text = format!(
        "{}\n{big}\n{}\n",
        claude_usage_line("m1", 111),
        claude_usage_line("m2", 222)
    );
    std::fs::write(&file, text).unwrap();
    let r = read_current(&loc, cache.path(), "claude", Some(SID)).unwrap();
    assert_eq!(r.tokens, Some(222));

    // With no usable line after it, the window doubles across the 5 MiB line
    // to the first one.
    let text = format!(
        "{}\n{big}\n{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n",
        claude_usage_line("m1", 111)
    );
    std::fs::write(&file, text).unwrap();
    let r = read_current(&loc, cache.path(), "claude", Some(SID)).unwrap();
    assert_eq!(r.tokens, Some(111));

    // 20 MiB of filler lines, then 1 usable last line: 1 window of 256 KiB.
    let mut f = std::fs::File::create(&file).unwrap();
    let filler = format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}\n",
        "y".repeat(1000)
    );
    let mut written = 0;
    while written < 20 << 20 {
        f.write_all(filler.as_bytes()).unwrap();
        written += filler.len();
    }
    writeln!(f, "{}", claude_usage_line("m3", 333)).unwrap();
    drop(f);
    let tail = scan_tail(&file, "claude").unwrap();
    assert_eq!(tail.usable.as_ref().map(|u| u.tokens), Some(333));
    assert!(tail.bytes_read <= 256 << 10, "{}", tail.bytes_read);
    let r = read_current(&loc, cache.path(), "claude", Some(SID)).unwrap();
    assert_eq!(r.tokens, Some(333));
}

#[test]
fn ctx_26_marker_cache_reads_only_new_bytes() {
    let (home, file) = claude_home(PENDING);
    let loc = Locations::under_home(home.path(), &Inherited::default());
    let cache = tempfile::tempdir().unwrap();
    let source = fixture_loc()
        .claude_projects
        .join(format!("-fixture/{PENDING}.jsonl"));
    std::fs::copy(&source, &file).unwrap();
    let first_len = std::fs::metadata(&file).unwrap().len();

    let r = read_current(&loc, cache.path(), "claude", Some(PENDING)).unwrap();
    assert!(r.pending);
    let c = marker_cache(cache.path());
    assert_eq!(c["v"], 1);
    assert_eq!(c["path"], file.to_str().unwrap());
    assert_eq!(c["scanned_to"], first_len);
    assert_eq!(c["marks"].as_array().unwrap().len(), 1);
    let offset = c["marks"][0]["offset"].as_u64().unwrap();
    assert_eq!(r.last_compaction.as_ref().unwrap().offset, offset);

    // A planted second mark in the old bytes of the cache is kept: proof that
    // the old bytes are not scanned again.
    let mut planted = c.clone();
    let mut extra = planted["marks"][0].clone();
    extra["offset"] = 1.into();
    planted["marks"]
        .as_array_mut()
        .unwrap()
        .insert(0, extra.clone());
    let cache_file = std::fs::read_dir(cache.path().join("markers"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    std::fs::write(&cache_file, serde_json::to_vec(&planted).unwrap()).unwrap();

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    writeln!(f, "{}", claude_usage_line("msg_C", 4321)).unwrap();
    drop(f);
    let second_len = std::fs::metadata(&file).unwrap().len();
    let r = read_current(&loc, cache.path(), "claude", Some(PENDING)).unwrap();
    assert_eq!(r.tokens, Some(4321));
    assert!(!r.pending);
    let c = marker_cache(cache.path());
    assert_eq!(c["scanned_to"], second_len);
    assert_eq!(c["marks"].as_array().unwrap().len(), 2);
    assert_eq!(r.marks, 2);

    // Truncated below `scanned_to`: rescan from 0, the planted mark is gone.
    let text = std::fs::read(&source).unwrap();
    let cut = text.len() - 1;
    let cut = text[..cut].iter().rposition(|b| *b == b'\n').unwrap() + 1;
    std::fs::write(&file, &text[..cut]).unwrap();
    let r = read_current(&loc, cache.path(), "claude", Some(PENDING)).unwrap();
    let c = marker_cache(cache.path());
    assert_eq!(c["scanned_to"], cut as u64);
    assert_eq!(c["marks"].as_array().unwrap().len(), 1);
    assert_eq!(r.marks, 1);
    assert!(r.pending);
}
