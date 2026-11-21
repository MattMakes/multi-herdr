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
}

const STDIN_LIMIT: u64 = 64 * 1024 * 1024;

pub const HYDRATE_LIMIT: usize = 8_000;

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
        HookEvent::PostCompact => post_compact(&src, record_id, now),
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

fn pre_compact(src: &Sources, record_id: &str, input: &str) -> Result<Option<String>> {
    if has_instructions(input) {
        return Ok(None);
    }
    let Some(record) = live_record(src, record_id)? else {
        return Ok(None);
    };
    message(src, &record, "instructions", "instructions-orchestrator").map(Some)
}

fn has_instructions(input: &str) -> bool {
    serde_json::from_str::<Value>(input)
        .ok()
        .and_then(|v| {
            v.get("custom_instructions")
                .and_then(Value::as_str)
                .map(|s| !s.trim().is_empty())
        })
        .unwrap_or(false)
}

fn post_compact(src: &Sources, record_id: &str, now: SystemTime) -> Result<Option<String>> {
    let Some(record) = live_record(src, record_id)? else {
        return Ok(None);
    };
    let path = record_workdir(src.ctx, &record).join(policy::handoff_path(&record.role));
    match fresh_handoff(&path, &record, now) {
        Some(text) => {
            let head = message(src, &record, "hydrate", "hydrate-orchestrator")?;
            Ok(Some(format!("{head}\n\n{}", cut(&text, &path))))
        }
        None => message(src, &record, "hydrate-missing", "hydrate-missing").map(Some),
    }
}

fn fresh_handoff(path: &Path, record: &Record, now: SystemTime) -> Option<String> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let asked = policy::asked_at(&record.history);
    policy::handoff_fresh(meta.modified().ok(), asked, now).ok()?;
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    (!text.is_empty()).then_some(text)
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
    fn ctx_29_pre_compact_is_silent_when_the_command_carries_instructions() {
        assert!(has_instructions(
            r#"{"trigger":"manual","custom_instructions":"Keep X"}"#
        ));
        assert!(!has_instructions(
            r#"{"trigger":"auto","custom_instructions":null}"#
        ));
        assert!(!has_instructions(
            r#"{"trigger":"manual","custom_instructions":"  "}"#
        ));
        assert!(!has_instructions("not json"));
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

        fn handoff_file(w: &World, id: &str) -> std::path::PathBuf {
            let record = w.ledger.get(id).unwrap();
            record_workdir(&w.ctx, &record).join(HANDOFF)
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
        fn ctx_29_pre_compact_prints_the_keep_list_for_a_bare_compaction() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            w.orchestrator("o-1", Some(SMALL));
            let auto = r#"{"trigger":"auto","custom_instructions":null}"#;
            let worker = pre_compact(&w.sources(), "r-1", auto).unwrap().unwrap();
            assert!(
                worker.starts_with("Keep in the summary: your role sonnet-1,"),
                "{worker}"
            );
            assert!(worker.ends_with(&format!("The full state is in {HANDOFF}.")));
            let orchestrator = pre_compact(&w.sources(), "o-1", auto).unwrap().unwrap();
            assert!(
                orchestrator.starts_with("Keep in the summary: your actual run slug,"),
                "{orchestrator}"
            );
            let typed = r#"{"trigger":"manual","custom_instructions":"Keep in the summary: x"}"#;
            assert_eq!(pre_compact(&w.sources(), "r-1", typed).unwrap(), None);
        }

        #[test]
        fn ctx_30_post_compact_prints_a_fresh_handoff_or_says_it_is_missing() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let now = SystemTime::now();
            let missing = post_compact(&w.sources(), "r-1", now).unwrap().unwrap();
            assert!(
                missing.starts_with("NOTE: Handoff. This session was compacted with no current"),
                "{missing}"
            );
            assert!(!missing.contains('\n'));
            let file = handoff_file(&w, "r-1");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "# What is next\nRun step 4.\n").unwrap();
            let fresh = post_compact(&w.sources(), "r-1", now).unwrap().unwrap();
            assert_eq!(
                fresh,
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
            let stale = post_compact(&w.sources(), "r-1", now).unwrap().unwrap();
            assert!(stale.starts_with("NOTE: Handoff. This session was compacted with no current"));
        }

        #[test]
        fn ctx_30_the_orchestrator_gets_its_own_first_line() {
            let w = World::new();
            w.orchestrator("o-1", Some(SMALL));
            let record = w.ledger.get("o-1").unwrap();
            let file =
                record_workdir(&w.ctx, &record).join("ai_docs/handoffs/orchestrator-whats-next.md");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "run slug: x").unwrap();
            let text = post_compact(&w.sources(), "o-1", SystemTime::now())
                .unwrap()
                .unwrap();
            assert!(text.contains("rebuild roster and ownership from horch ledger status"));
            assert!(text.ends_with("\n\nrun slug: x"));
        }
    }
}
