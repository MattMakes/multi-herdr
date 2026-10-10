//! `multi-herdr-dataset judge-job`: one detached judge attempt (B4, JDG-04).
//!
//! The coordinator starts it through `evaluation::scheduler::schedule`. It
//! runs the hidden `judge` teammate headless in the sealed bundle, and writes
//! only its job dir: `heartbeat` while it runs, then `output.json` (the
//! answer text, ≤ 1 MiB) and `exit.json`. It emits no event and writes no
//! judgment: the coordinator is the single authority (CMP-16).

use std::io::{Read, Write};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use horch_core::evaluation::rubric::schema_text;
use horch_core::evaluation::scheduler::{
    ExitReason, JobExit, EXIT_FILE, OUTPUT_CAP_BYTES, OUTPUT_FILE,
};
use horch_core::fsx;
use horch_core::harness::headless::{headless_command, judge_prompt, supports_json_schema};
use horch_core::heartbeat;
use horch_core::ids::{RoundId, SessionId};
use horch_core::roster::Roster;
use horch_core::runtime::RuntimeContext;

use super::cli::JudgeJobArgs;
use super::exit;

/// Inside the job, after the judge answered and before `output.json`.
pub(crate) const ABORT_BEFORE_OUTPUT: &str = "abort-in-judge-job-before-output";
/// Inside the job, after `output.json` and before `exit.json`.
pub(crate) const ABORT_AFTER_OUTPUT: &str = "abort-after-judge-output";

/// What one judge CLI run gave.
enum RunResult {
    /// The answer text, taken out of the `--output-format json` envelope.
    Answer(Vec<u8>),
    Failed(JobExit),
}

pub(crate) fn judge_job(ctx: &RuntimeContext, args: &JudgeJobArgs) -> Result<u8> {
    let round: RoundId = args.round.parse().context("--round")?;
    let job_dir = super::dataset_paths(ctx)?.job_dir(&round, args.attempt)?;
    if !job_dir.is_dir() {
        bail!(
            "{} does not exist; the coordinator creates it",
            job_dir.display()
        );
    }
    if job_dir.join(EXIT_FILE).exists() || job_dir.join(OUTPUT_FILE).exists() {
        bail!("attempt {} of round {round} has run already", args.attempt);
    }

    let beat = heartbeat::start(&job_dir);
    let result = run_judge(ctx, args).unwrap_or_else(|e| {
        eprintln!("judge-job: {e:#}");
        RunResult::Failed(JobExit {
            reason: ExitReason::Crash,
            code: None,
        })
    });
    let done = match result {
        RunResult::Answer(text) => {
            ctx.settings.faults.abort_if(ABORT_BEFORE_OUTPUT);
            fsx::create_immutable(&job_dir.join(OUTPUT_FILE), &text, fsx::PRIVATE_FILE)?;
            ctx.settings.faults.abort_if(ABORT_AFTER_OUTPUT);
            JobExit {
                reason: ExitReason::Ok,
                code: Some(0),
            }
        }
        RunResult::Failed(e) => e,
    };
    let bytes = serde_json::to_vec(&done)?;
    fsx::create_immutable(&job_dir.join(EXIT_FILE), &bytes, fsx::PRIVATE_FILE)?;
    beat.stop();
    Ok(if done.reason == ExitReason::Ok {
        exit::SUCCESS
    } else {
        exit::FAILURE
    })
}

fn run_judge(ctx: &RuntimeContext, args: &JudgeJobArgs) -> Result<RunResult> {
    let roster = Roster::load_layered(
        ctx.inherited.home_var.as_deref().map(Path::new),
        ctx.bins.roster_override.as_deref(),
        None,
    )?;
    warn(&roster, &mut std::io::stderr());
    let mut judge = roster.require("judge")?.clone();
    // The coordinator digested these into judge_policy_digest; they win.
    judge.model = Some(args.model.clone());
    judge.effort = Some(args.effort.clone());
    let session = SessionId::new(args.session_id.clone()).context("--session-id")?;
    let schema = supports_json_schema(&ctx.bins.harness.claude).then(schema_text);
    let mut cmd = headless_command(ctx, &judge, &session, schema)?;
    cmd.current_dir(&args.input_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {}", ctx.bins.harness.claude.display()))?;

    let prompt = judge_prompt(&judge);
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let over_cap = Arc::new(AtomicBool::new(false));
    let mut stdout = child.stdout.take().expect("piped stdout");
    let flag = over_cap.clone();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // The envelope adds a little to the answer; allow it.
                    if out.len() + n > OUTPUT_CAP_BYTES * 2 {
                        flag.store(true, Ordering::SeqCst);
                        break;
                    }
                    out.extend_from_slice(&buf[..n]);
                }
            }
        }
        out
    });

    let deadline = Instant::now() + Duration::from_secs(args.timeout_s);
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let failed = if over_cap.load(Ordering::SeqCst) {
            Some(ExitReason::TooLarge)
        } else if Instant::now() >= deadline {
            Some(ExitReason::Timeout)
        } else {
            None
        };
        if let Some(reason) = failed {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(RunResult::Failed(JobExit { reason, code: None }));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let raw = reader.join().unwrap_or_default();
    if over_cap.load(Ordering::SeqCst) {
        return Ok(RunResult::Failed(JobExit {
            reason: ExitReason::TooLarge,
            code: status.code(),
        }));
    }
    if !status.success() {
        return Ok(RunResult::Failed(JobExit {
            reason: ExitReason::Crash,
            code: status.code(),
        }));
    }
    let Some(text) = answer_text(&raw) else {
        return Ok(RunResult::Failed(JobExit {
            reason: ExitReason::Crash,
            code: status.code(),
        }));
    };
    if text.len() > OUTPUT_CAP_BYTES {
        return Ok(RunResult::Failed(JobExit {
            reason: ExitReason::TooLarge,
            code: status.code(),
        }));
    }
    Ok(RunResult::Answer(text))
}

/// The answer in a `--output-format json` envelope: `structured_output` when
/// `--json-schema` produced one, else the `result` text. `None` for an error
/// envelope. Stdout that is not an envelope is passed on as it is, so the
/// coordinator's strict parser records it as malformed.
/// Write each [`Roster::load_warnings`] line to `out` as `warning: <line>`,
/// as `horch`'s `load_roster` does. A job loads the roster once.
fn warn(roster: &Roster, out: &mut impl Write) {
    for w in roster.load_warnings() {
        let _ = writeln!(out, "warning: {w}");
    }
}

fn answer_text(raw: &[u8]) -> Option<Vec<u8>> {
    let Ok(serde_json::Value::Object(env)) = serde_json::from_slice(raw) else {
        return Some(raw.to_vec());
    };
    if env.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(v) = env.get("structured_output").filter(|v| !v.is_null()) {
        return serde_json::to_vec(v).ok();
    }
    match env.get("result") {
        Some(serde_json::Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => Some(raw.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F7: a teammate file that does not parse gives one `warning:` line
    /// in the judge job, and the judge still loads.
    #[test]
    fn roster_warnings_are_printed() {
        let tmp = tempfile::tempdir().unwrap();
        let opus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/opus.md");
        let text = std::fs::read_to_string(opus)
            .unwrap()
            .replace("name: opus", "name: newcomer")
            .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1);
        std::fs::write(tmp.path().join("newcomer.md"), text).unwrap();
        let roster = Roster::load_layered(None, Some(tmp.path()), None).unwrap();
        assert!(roster.require("judge").is_ok());
        let mut out = Vec::new();
        warn(&roster, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("warning: teammate 'newcomer'"), "{text}");
    }

    #[test]
    fn answer_text_reads_the_envelope() {
        let env = br#"{"type":"result","is_error":false,"result":"{\"a\":1}","session_id":"s"}"#;
        assert_eq!(answer_text(env).unwrap(), br#"{"a":1}"#);
        let structured = br#"{"result":"","structured_output":{"a":2}}"#;
        assert_eq!(answer_text(structured).unwrap(), br#"{"a":2}"#);
        assert_eq!(answer_text(br#"{"is_error":true,"result":"x"}"#), None);
        assert_eq!(answer_text(b"not json").unwrap(), b"not json");
    }
}
