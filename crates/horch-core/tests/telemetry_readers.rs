//! The readers against the fixture corpus. Every expected number comes from
//! `tests/fixtures/telemetry/EXPECTED.md`, computed by hand.

mod common;

use std::collections::BTreeMap;
use std::io::Write;

use common::*;
use horch_core::telemetry::readers::{poll_record, read_whole, Cursors, Unreadable};
use horch_core::telemetry::{Observation, QuotaSignal, RawUsage, TokenClasses};
use horch_core::usage::Locations;

fn t(input: u64, cw5: u64, cw1: u64, cr: u64, out: u64, reas: u64) -> TokenClasses {
    TokenClasses {
        input,
        cache_write_5m: cw5,
        cache_write_1h: cw1,
        cache_read: cr,
        output: out,
        reasoning: reas,
    }
}

fn sum(events: &[RawUsage]) -> TokenClasses {
    let mut s = TokenClasses::default();
    for e in events {
        s.add(&e.tokens);
    }
    s
}

fn whole(w: &World, agent: &str, sid: &str) -> Vec<RawUsage> {
    read_whole(&w.loc, agent, Some(sid)).unwrap().0
}

fn observations(loc: &Locations, agent: &str, sid: &str) -> Vec<Observation> {
    poll_record(loc, agent, Some(sid), &mut Cursors::new())
        .unwrap()
        .observations
}

#[test]
fn tel_03_classes_per_agent() {
    let w = world(Part::Whole);
    assert_eq!(
        sum(&whole(&w, "claude", O1)),
        t(177, 1900, 2000, 156000, 695, 120)
    );
    assert_eq!(
        sum(&whole(&w, "claude", A2)),
        t(100, 0, 2502, 184363, 1710, 300)
    );
    assert_eq!(sum(&whole(&w, "codex", A3)), t(300, 0, 0, 1200, 300, 70));
    assert_eq!(sum(&whole(&w, "codex", A4)), t(1000, 0, 0, 3000, 600, 200));
    assert_eq!(sum(&whole(&w, "codex", A5)), t(800, 0, 0, 2200, 450, 150));
    assert_eq!(sum(&whole(&w, "pi", B1)), t(1030, 500, 300, 2000, 210, 10));
    assert_eq!(
        sum(&whole(&w, "prime", &prime_sid(&w.state))),
        t(70, 200, 0, 2200, 70, 0)
    );
    // pi: cacheWrite1h is split out of cacheWrite, and the provider prefixes
    // the model so the price table can find it.
    let pi = whole(&w, "pi", B1);
    let m4 = pi.iter().find(|e| e.event_id == "m4").unwrap();
    assert_eq!(
        (m4.tokens.cache_write_5m, m4.tokens.cache_write_1h),
        (500, 300)
    );
    assert_eq!(m4.model, "anthropic/claude-sonnet-5");
    let m3 = pi.iter().find(|e| e.event_id == "m3").unwrap();
    assert!(m3.tool_nested, "toolResult usage is marked");
    assert!(m3.model.is_empty(), "the record's model stands in later");
}

#[test]
fn tel_04_claude_dedupes_message_id() {
    let w = world(Part::Whole);
    let ev = whole(&w, "claude", O1);
    let main: Vec<&RawUsage> = ev.iter().filter(|e| !e.subagent).collect();
    let ids: Vec<&str> = main.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["msg_a", "msg_b", "msg_c"],
        "one event per message.id"
    );
    let a = main[0];
    assert_eq!(a.tokens, t(10, 0, 2000, 50000, 300, 120));
    assert_eq!(a.ts, "2026-09-28T17:00:01Z");
    // No cache_creation object: every written token is 5m.
    assert_eq!(main[2].tokens.cache_write_5m, 400);
}

#[test]
fn tel_04_claude_skips_synthetic() {
    let w = world(Part::Whole);
    let obs = observations(&w.loc, "claude", O1);
    assert!(!obs
        .iter()
        .any(|o| matches!(o, Observation::Usage(u) if u.model == "<synthetic>")));
    let refusals: Vec<&QuotaSignal> = obs
        .iter()
        .filter_map(|o| match o {
            Observation::Quota(q) => Some(q),
            _ => None,
        })
        .collect();
    assert_eq!(
        refusals,
        vec![&QuotaSignal::Refusal {
            pool: "claude".into(),
            at: "2026-09-28T17:05:00Z".into(),
            what: "7d".into(),
        }]
    );
}

#[test]
fn tel_04_claude_counts_subagents() {
    let w = world(Part::Whole);
    let ev = whole(&w, "claude", O1);
    let sub: Vec<&RawUsage> = ev.iter().filter(|e| e.subagent).collect();
    let ids: Vec<&str> = sub.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["msg_s1", "msg_s1+1", "msg_s2", "msg_s3"],
        "nested dir included"
    );
    assert_eq!(sub[0].tokens.output, 20);
    assert!(sub[1].delta);
    assert_eq!(sub[1].tokens, t(0, 0, 0, 0, 40, 0), "only the growth");
    let mut s = TokenClasses::default();
    for e in &sub {
        s.add(&e.tokens);
    }
    assert_eq!(s, t(160, 500, 0, 1000, 95, 0));
}

#[test]
fn tel_05_codex_counts_final_response() {
    let w = world(Part::Whole);
    let ev = whole(&w, "codex", A3);
    let ids: Vec<&str> = ev.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(ids, vec!["resp_1", "resp_2"]);
    let s = sum(&ev);
    // input + cached + output, as Codex counts total_tokens.
    let records_total = s.input + s.cache_read + s.output;
    assert_eq!(records_total, 1800, "the last thread_token_usage");
    assert!(records_total > 1200, "more than the last token_count total");
    assert!(ev
        .iter()
        .all(|e| e.model == "gpt-5.6-sol" && e.effort.as_deref() == Some("medium")));
    let obs = observations(&w.loc, "codex", A3);
    assert!(obs.iter().any(|o| matches!(o, Observation::Quota(QuotaSignal::Refusal { what, .. }) if what == "usage_limit_exceeded")));
    assert!(obs.iter().any(
        |o| matches!(o, Observation::Quota(QuotaSignal::Snapshot { windows, .. })
        if windows.len() == 1 && windows[0].name == "7d" && (windows[0].used - 0.99).abs() < 1e-9)
    ));
}

#[test]
fn tel_05_codex_fallback_without_records() {
    let w = world(Part::Whole);
    let ev = whole(&w, "codex", A5);
    let ids: Vec<&str> = ev.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["tc:2300", "tc:3450"],
        "each total written twice, counted once"
    );
    assert_eq!(ev[0].tokens, t(500, 0, 0, 1500, 300, 100));
    assert_eq!(ev[1].tokens, t(300, 0, 0, 700, 150, 50));
    assert_eq!(whole(&w, "codex", A4).len(), 1);
}

#[test]
fn tel_06_opencode_reads_completed_messages() {
    if !need_sqlite("tel_06_opencode_reads_completed_messages") {
        return;
    }
    let w = world(Part::Whole);
    let ev = whole(&w, "opencode", "ses_beta1");
    let ids: Vec<&str> = ev.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["msg_o1", "msg_o4"],
        "incomplete, user and foreign messages skipped"
    );
    assert_eq!(sum(&ev), t(1500, 0, 0, 200, 400, 40));
    assert_eq!(ev[0].model, "opencode/nemotron-3-ultra-free");
    assert_eq!(ev[0].harness_cost, Some(0.0));
    assert_eq!(ev[0].ts, "2026-09-28T12:53:25Z");
}

#[test]
fn tel_06_opencode_update_emits_delta() {
    if !need_sqlite("tel_06_opencode_update_emits_delta") {
        return;
    }
    let w = world(Part::Whole);
    let mut cursors = Cursors::new();
    let first = poll_record(&w.loc, "opencode", Some("ses_beta1"), &mut cursors).unwrap();
    assert_eq!(first.observations.len(), 2);
    // Nothing new: nothing emitted.
    let again = poll_record(&w.loc, "opencode", Some("ses_beta1"), &mut cursors).unwrap();
    assert!(again.observations.is_empty(), "{:?}", again.observations);
    let db = &w.loc.opencode_db;
    let sql = r#"UPDATE message SET time_updated = 1790600040000, data = json_set(data, '$.tokens.output', 160) WHERE id = 'msg_o4';"#;
    let ok = std::process::Command::new(sqlite3().unwrap())
        .arg(db)
        .arg(sql)
        .status()
        .unwrap();
    assert!(ok.success());
    let after = poll_record(&w.loc, "opencode", Some("ses_beta1"), &mut cursors).unwrap();
    let ev: Vec<RawUsage> = after
        .observations
        .into_iter()
        .filter_map(|o| match o {
            Observation::Usage(u) => Some(u),
            _ => None,
        })
        .collect();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].event_id, "msg_o4+1");
    assert!(ev[0].delta);
    assert_eq!(ev[0].tokens, t(0, 0, 0, 0, 60, 0));
}

#[test]
fn tel_01_unread_has_reason() {
    let w = world(Part::Whole);
    let mut c = Cursors::new();
    assert_eq!(
        poll_record(&w.loc, "claude", None, &mut c).unwrap_err(),
        Unreadable::NoSessionId
    );
    assert!(matches!(
        poll_record(&w.loc, "claude", Some("missing-sid"), &mut c).unwrap_err(),
        Unreadable::NoTranscript(_)
    ));
    assert_eq!(
        poll_record(&w.loc, "none", Some("smoke-sid"), &mut c)
            .unwrap_err()
            .to_string(),
        "agent none is not read"
    );
    // opencode without a database file.
    let empty = locations(&w.home.join("nowhere"));
    assert!(poll_record(&empty, "opencode", Some("ses_beta1"), &mut c).is_err());
}

/// Every input file of the corpus, with the record it belongs to.
fn inputs(w: &World) -> Vec<(&'static str, String, std::path::PathBuf)> {
    let claude = w.home.join(".claude/projects/-work-alpha");
    let codex = w.home.join(".codex/sessions/2026");
    vec![
        ("claude", O1.to_string(), claude.join(format!("{O1}.jsonl"))),
        (
            "claude",
            O1.to_string(),
            claude.join(format!("{O1}/subagents/agent-a1.jsonl")),
        ),
        (
            "claude",
            O1.to_string(),
            claude.join(format!("{O1}/subagents/workflow-x/agent-a2.jsonl")),
        ),
        ("claude", A2.to_string(), claude.join(format!("{A2}.jsonl"))),
        (
            "codex",
            A3.to_string(),
            codex.join(format!("09/27/rollout-2026-09-27T20-44-04-{A3}.jsonl")),
        ),
        (
            "codex",
            A5.to_string(),
            codex.join(format!("09/11/rollout-2026-09-11T09-59-00-{A5}.jsonl")),
        ),
        (
            "codex",
            A4.to_string(),
            codex.join(format!("09/12/rollout-2026-09-12T07-59-00-{A4}.jsonl")),
        ),
        (
            "pi",
            B1.to_string(),
            w.home.join(format!(
                ".pi/agent/sessions/--work-beta--/2026-09-28T12-53-00-000Z_{B1}.jsonl"
            )),
        ),
        (
            "prime",
            prime_sid(&w.state),
            std::path::PathBuf::from(prime_sid(&w.state)),
        ),
    ]
}

/// The events of `obs`, deduplicated the way the store does, as a map.
fn keyed(obs: Vec<Observation>, into: &mut BTreeMap<String, RawUsage>) {
    for o in obs {
        if let Observation::Usage(u) = o {
            into.entry(u.event_id.clone()).or_insert(u);
        }
    }
}

#[test]
fn tel_07_append_in_stages_equals_whole() {
    let w = world(Part::Whole);
    for (agent, sid, path) in inputs(&w) {
        let full = std::fs::read(&path).unwrap();
        let mut expected = BTreeMap::new();
        keyed(observations(&w.loc, agent, &sid), &mut expected);

        // Cut points: every line boundary, and 3 offsets inside lines.
        let mut cuts: Vec<usize> = full
            .iter()
            .enumerate()
            .filter(|(_, b)| **b == b'\n')
            .map(|(i, _)| i + 1)
            .collect();
        for frac in [3, 2] {
            cuts.push(full.len() / frac + 1);
        }
        cuts.push(7.min(full.len()));
        cuts.sort();
        cuts.dedup();

        let mut cursors = Cursors::new();
        let mut got = BTreeMap::new();
        std::fs::write(&path, b"").unwrap();
        let mut written = 0;
        for cut in cuts.into_iter().chain([full.len()]) {
            if cut > written {
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap();
                f.write_all(&full[written..cut]).unwrap();
                written = cut;
            }
            let polled = poll_record(&w.loc, agent, Some(&sid), &mut cursors).unwrap();
            keyed(polled.observations, &mut got);
        }
        assert_eq!(got, expected, "{}", path.display());
    }
}

#[test]
fn tel_07_restart_resumes() {
    let half = world(Part::FirstHalf);
    let mut cursors = Cursors::new();
    let mut got = BTreeMap::new();
    keyed(
        observations_with(&half.loc, "claude", O1, &mut cursors),
        &mut got,
    );
    // Serialize the cursors, as the collector does between runs.
    let saved = serde_json::to_string(&cursors).unwrap();
    // Grow every file to its whole content, then resume.
    for (from, to) in targets(&half.home, &half.state) {
        copy(&from, &to, Part::Whole);
    }
    let mut cursors: Cursors = serde_json::from_str(&saved).unwrap();
    keyed(
        observations_with(&half.loc, "claude", O1, &mut cursors),
        &mut got,
    );
    let w = world(Part::Whole);
    let mut expected = BTreeMap::new();
    keyed(observations(&w.loc, "claude", O1), &mut expected);
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        got.values().map(|u| u.tokens).collect::<Vec<_>>(),
        expected.values().map(|u| u.tokens).collect::<Vec<_>>()
    );
}

fn observations_with(loc: &Locations, agent: &str, sid: &str, c: &mut Cursors) -> Vec<Observation> {
    poll_record(loc, agent, Some(sid), c).unwrap().observations
}

#[test]
fn tel_11_readers_copy_no_content() {
    let w = world(Part::Whole);
    let mut text = String::new();
    for (agent, sid, _) in inputs(&w) {
        for o in observations(&w.loc, agent, &sid) {
            text.push_str(&format!("{o:?}\n"));
        }
    }
    for sentinel in ["SENTINEL", "sentinel@", "acct_", "You've hit"] {
        assert!(!text.contains(sentinel), "{sentinel} leaked:\n{text}");
    }
}
