use std::io::Read;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::process::ExitCode;
use std::time::SystemTime;

use anyhow::Result;
use clap::ValueEnum;
use horch_core::compaction::policy;
use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::prompts;
use horch_core::runtime::RuntimeContext;
use horch_core::usage::Locations;
use serde_json::{json, Value};

use super::context::{self, record_workdir, Sources};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookEvent {
    Context,
    PreCompact,
    PostCompact,
    SessionStart,
}

const STDIN_LIMIT: u64 = 64 * 1024 * 1024;

pub const HYDRATE_LIMIT: usize = 8_000;

pub const SUMMARY_MARK: &str = "<!-- horch: saved from the compaction summary -->";

pub const EVENT_SAVED: &str = "whats-next-saved";

pub fn hook(ctx: &RuntimeContext, event: HookEvent) -> ExitCode {
    let input = read_input();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = std::panic::catch_unwind(AssertUnwindSafe(|| {
        output(ctx, event, &input, SystemTime::now()).ok().flatten()
    }));
    std::panic::set_hook(previous);
    if let Ok(Some(text)) = out {
        println!("{text}");
    }
    ExitCode::SUCCESS
}

fn read_input() -> String {
    let mut bytes = Vec::new();
    let _ = std::io::stdin().take(STDIN_LIMIT).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(crate) fn output(
    ctx: &RuntimeContext,
    event: HookEvent,
    input: &str,
    now: SystemTime,
) -> Result<Option<String>> {
    let Some(record_id) = ctx.worker.as_ref().and_then(|w| w.record_id.as_deref()) else {
        return Ok(None);
    };
    let ledger = SessionLedger::open_in(ctx)?;
    let roster = super::load_roster_unwarned(ctx, None)
        .or_else(|_| horch_core::roster::Roster::builtin())?;
    let loc = Locations::from_context(ctx);
    let cache = context::cache_dir(ctx);
    let src = Sources {
        ctx,
        ledger: &ledger,
        roster: &roster,
        loc: &loc,
        cache_dir: &cache,
    };
    match event {
        HookEvent::Context => Ok(super::messaging::hook_check(&src, record_id)?.map(batch_context)),
        HookEvent::PreCompact => pre_compact(&src, record_id, input),
        HookEvent::PostCompact => post_compact(&src, record_id, input, now).map(|_| None),
        HookEvent::SessionStart => session_start(&src, record_id, now),
    }
}

fn batch_context(line: String) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolBatch",
            "additionalContext": line,
        }
    })
    .to_string()
}

fn live_record(src: &Sources, record_id: &str) -> Result<Option<Record>> {
    let record = src.ledger.get(record_id)?;
    Ok(record.execution_status().is_live().then_some(record))
}

fn message(src: &Sources, record: &Record, worker: &str, orchestrator: &str) -> Result<String> {
    let key = if record.is_orchestrator() {
        orchestrator
    } else {
        worker
    };
    let handoff = policy::handoff_path(&record.role);
    let vars = [
        ("role", record.role.as_str()),
        ("handoff", handoff.as_str()),
    ]
    .into_iter()
    .collect();
    prompts::context_message(src.roster, key, &vars)
}

fn input_text(input: &str, key: &str) -> Option<String> {
    let value: Value = serde_json::from_str(input).ok()?;
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn pre_compact(src: &Sources, record_id: &str, input: &str) -> Result<Option<String>> {
    let Some(record) = live_record(src, record_id)? else {
        return Ok(None);
    };
    let format = message(src, &record, "summary-format", "summary-format")?;
    if input_text(input, "custom_instructions").is_some() {
        return Ok(Some(format));
    }
    let keep = message(src, &record, "instructions", "instructions-orchestrator")?;
    Ok(Some(format!("{keep}\n{format}")))
}

enum Handoff {
    Written(String),
    FromSummary,
    Absent,
}

fn handoff_state(path: &Path, record: &Record, now: SystemTime) -> Handoff {
    let Some(meta) = std::fs::metadata(path).ok().filter(|m| m.is_file()) else {
        return Handoff::Absent;
    };
    let text = std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_default();
    if text.starts_with(SUMMARY_MARK) {
        return Handoff::FromSummary;
    }
    let asked = policy::asked_at(&record.history);
    let fresh = policy::handoff_fresh(meta.modified().ok(), asked, now).is_ok();
    if fresh && !text.is_empty() {
        Handoff::Written(text)
    } else {
        Handoff::Absent
    }
}

fn handoff_file(src: &Sources, record: &Record) -> std::path::PathBuf {
    record_workdir(src.ctx, record).join(policy::handoff_path(&record.role))
}

fn post_compact(src: &Sources, record_id: &str, input: &str, now: SystemTime) -> Result<bool> {
    let Some(record) = live_record(src, record_id)? else {
        return Ok(false);
    };
    let Some(summary) = input_text(input, "compact_summary") else {
        return Ok(false);
    };
    let path = handoff_file(src, &record);
    if matches!(handoff_state(&path, &record, now), Handoff::Written(_)) {
        return Ok(false);
    }
    let body = summary_body(&summary);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    horch_core::fsx::write_atomic(&path, format!("{SUMMARY_MARK}\n{body}\n").as_bytes(), 0o644)?;
    let trigger = input_text(input, "trigger").unwrap_or_else(|| "unknown".into());
    let _ = src.ledger.record_event(
        record_id,
        EVENT_SAVED,
        &format!(
            "{} from the {trigger} compaction summary; {} characters",
            policy::handoff_path(&record.role),
            body.chars().count()
        ),
    );
    Ok(true)
}

fn summary_body(summary: &str) -> String {
    let after = summary
        .split_once("</analysis>")
        .map_or(summary, |(_, rest)| rest)
        .trim();
    let inner = after.strip_prefix("<summary>").unwrap_or(after);
    inner
        .strip_suffix("</summary>")
        .unwrap_or(inner)
        .trim()
        .to_string()
}

fn session_start(src: &Sources, record_id: &str, now: SystemTime) -> Result<Option<String>> {
    let Some(record) = live_record(src, record_id)? else {
        return Ok(None);
    };
    let path = handoff_file(src, &record);
    match handoff_state(&path, &record, now) {
        Handoff::Written(text) => {
            let head = message(src, &record, "hydrate", "hydrate-orchestrator")?;
            Ok(Some(format!("{head}\n\n{}", cut(&text, &path))))
        }
        Handoff::FromSummary | Handoff::Absent => message(
            src,
            &record,
            "hydrate-summary",
            "hydrate-summary-orchestrator",
        )
        .map(Some),
    }
}

fn cut(text: &str, path: &Path) -> String {
    match text.char_indices().nth(HYDRATE_LIMIT) {
        None => text.to_string(),
        Some((end, _)) => format!(
            "{}\n\n[handoff cut at {HYDRATE_LIMIT} characters; read the rest at {}]",
            &text[..end],
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_31_the_summary_body_drops_the_analysis_and_the_tags() {
        let raw = "<analysis>\nthinking\n</analysis>\n\n<summary>\n1. Request: x\n</summary>";
        assert_eq!(summary_body(raw), "1. Request: x");
        assert_eq!(summary_body("  plain text  "), "plain text");
        assert_eq!(summary_body("<summary>a</summary>"), "a");
    }

    #[test]
    fn ctx_30_hydration_cuts_a_long_handoff_and_names_the_file() {
        let path = Path::new("/w/ai_docs/handoffs/opus-1-whats-next.md");
        let short = "next step: run the tests";
        assert_eq!(cut(short, path), short);
        let long = "é".repeat(HYDRATE_LIMIT + 5);
        let out = cut(&long, path);
        assert!(out.starts_with(&"é".repeat(HYDRATE_LIMIT)));
        assert!(out.ends_with(
            "[handoff cut at 8000 characters; read the rest at /w/ai_docs/handoffs/opus-1-whats-next.md]"
        ));
        assert!(out.chars().count() < 10_000);
    }

    mod in_a_world {
        use super::super::super::context::testkit::*;
        use super::super::*;

        const HANDOFF: &str = "ai_docs/handoffs/sonnet-1-whats-next.md";
        const AUTO: &str = r#"{"trigger":"auto","custom_instructions":null}"#;

        fn file_of(w: &World, id: &str) -> std::path::PathBuf {
            handoff_file(&w.sources(), &w.ledger.get(id).unwrap())
        }

        fn write(path: &Path, text: &str) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn summary_input(trigger: &str, summary: &str) -> String {
            json!({"trigger": trigger, "compact_summary": summary}).to_string()
        }

        #[test]
        fn ctx_28_the_hook_asks_once_with_the_request_line() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let first = super::super::super::messaging::hook_check(&w.sources(), "r-1")
                .unwrap()
                .expect("a session over its threshold is asked");
            assert!(
                first.starts_with(
                    "NOTE: Your context is 311225 tokens. Your threshold is 135000 tokens. \
                     At your next stopping point, write ai_docs/handoffs/sonnet-1-whats-next.md"
                ),
                "{first}"
            );
            let wrapped: Value = serde_json::from_str(&batch_context(first.clone())).unwrap();
            assert_eq!(
                wrapped,
                json!({"hookSpecificOutput": {
                    "hookEventName": "PostToolBatch",
                    "additionalContext": first,
                }})
            );
            let again = super::super::super::messaging::hook_check(&w.sources(), "r-1").unwrap();
            assert_eq!(again, None);
            assert_eq!(w.events("r-1", "context-warned").len(), 1);
        }

        #[test]
        fn ctx_28_the_hook_is_silent_under_the_threshold() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
            let line = super::super::super::messaging::hook_check(&w.sources(), "r-1").unwrap();
            assert_eq!(line, None);
            assert!(w.events("r-1", "context-warned").is_empty());
        }

        #[test]
        fn ctx_29_pre_compact_asks_for_a_whats_next_summary() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            w.orchestrator("o-1", Some(SMALL));
            let worker = pre_compact(&w.sources(), "r-1", AUTO).unwrap().unwrap();
            let (keep, format) = worker.split_once('\n').expect("2 lines");
            assert!(
                keep.starts_with("Keep in the summary: your role sonnet-1,"),
                "{keep}"
            );
            assert!(
                format.starts_with("Write the summary as the whats-next handoff of sonnet-1,"),
                "{format}"
            );
            assert!(format.contains("4. Next:"), "{format}");
            assert!(worker.chars().count() < 10_000);
            let orchestrator = pre_compact(&w.sources(), "o-1", AUTO).unwrap().unwrap();
            assert!(
                orchestrator.starts_with("Keep in the summary: your actual run slug,"),
                "{orchestrator}"
            );
            let typed = r#"{"trigger":"manual","custom_instructions":"Keep in the summary: x"}"#;
            let only_format = pre_compact(&w.sources(), "r-1", typed).unwrap().unwrap();
            assert_eq!(only_format, format);
        }

        #[test]
        fn ctx_31_post_compact_saves_the_summary_as_the_whats_next_file() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let now = SystemTime::now();
            let input = summary_input(
                "auto",
                "<analysis>a</analysis>\n<summary>\n4. Next: run step 4\n</summary>",
            );
            assert!(post_compact(&w.sources(), "r-1", &input, now).unwrap());
            let file = file_of(&w, "r-1");
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                format!("{SUMMARY_MARK}\n4. Next: run step 4\n")
            );
            assert_eq!(
                w.events("r-1", EVENT_SAVED),
                [format!(
                    "{HANDOFF} from the auto compaction summary; 19 characters"
                )]
            );
            let later = summary_input("manual", "4. Next: run step 5");
            assert!(post_compact(&w.sources(), "r-1", &later, now).unwrap());
            assert!(std::fs::read_to_string(&file)
                .unwrap()
                .ends_with("run step 5\n"));
            assert!(!post_compact(&w.sources(), "r-1", "{}", now).unwrap());
        }

        #[test]
        fn ctx_31_post_compact_keeps_a_handoff_the_session_wrote() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let file = file_of(&w, "r-1");
            write(&file, "# What is next\nRun step 4.\n");
            let input = summary_input("manual", "a summary");
            assert!(!post_compact(&w.sources(), "r-1", &input, SystemTime::now()).unwrap());
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                "# What is next\nRun step 4.\n"
            );
            assert!(w.events("r-1", EVENT_SAVED).is_empty());
        }

        #[test]
        fn ctx_30_session_start_gives_the_written_handoff_or_points_at_the_summary() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let now = SystemTime::now();
            let summary_line =
                "NOTE: Handoff. This session was compacted. The summary above is your \
                                whats-next.";
            let none = session_start(&w.sources(), "r-1", now).unwrap().unwrap();
            assert!(none.starts_with(summary_line), "{none}");
            assert!(!none.contains('\n'));
            let file = file_of(&w, "r-1");
            write(&file, "# What is next\nRun step 4.\n");
            let written = session_start(&w.sources(), "r-1", now).unwrap().unwrap();
            assert_eq!(
                written,
                format!(
                    "NOTE: Handoff. This session was compacted. Your handoff file {HANDOFF} \
                     follows. Continue your task from its next step. Do not repeat finished \
                     steps. Where the summary is newer than the file, the summary is correct.\
                     \n\n# What is next\nRun step 4."
                )
            );
            w.event_at(
                "r-1",
                "2099-01-01T00:00:00.000Z",
                "context-warned",
                "tokens 1 threshold 1",
            );
            let stale = session_start(&w.sources(), "r-1", now).unwrap().unwrap();
            assert!(stale.starts_with(summary_line), "{stale}");
        }

        #[test]
        fn ctx_30_a_summary_file_is_not_printed_a_second_time() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let now = SystemTime::now();
            let input = summary_input("auto", "4. Next: run step 4");
            assert!(post_compact(&w.sources(), "r-1", &input, now).unwrap());
            let text = session_start(&w.sources(), "r-1", now).unwrap().unwrap();
            assert!(!text.contains("run step 4"), "{text}");
            assert!(
                text.contains(&format!("horch saved it as {HANDOFF}")),
                "{text}"
            );
        }

        #[test]
        fn ctx_30_the_orchestrator_gets_its_own_first_line() {
            let w = World::new();
            w.orchestrator("o-1", Some(SMALL));
            let now = SystemTime::now();
            let pointed = session_start(&w.sources(), "o-1", now).unwrap().unwrap();
            assert!(pointed.contains("rebuild roster and ownership from horch ledger status"));
            let file = file_of(&w, "o-1");
            write(&file, "run slug: x");
            let text = session_start(&w.sources(), "o-1", now).unwrap().unwrap();
            assert!(text.contains("rebuild roster and ownership from horch ledger status"));
            assert!(text.ends_with("\n\nrun slug: x"));
        }
    }
}
