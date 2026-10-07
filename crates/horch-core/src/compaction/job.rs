//! The compaction job's state machine (CTX-13, CTX-15, CTX-22, CTX-23).
//!
//! [`run`] waits for the pane to be idle, types the compact line, confirms
//! the compaction by the transcript marker, waits for idle again, types the
//! resume line, and reports. Every effect goes through [`JobPorts`], so a
//! fake runs the whole job in a test with a fake clock.
//!
//! Every end records 1 event and sends 1 report: `compacted` and the
//! `reported` line on success, `compact-failed` and the `failed` line on a
//! failure. Their own errors go to the log only.

use std::time::Duration;

use anyhow::Result;

use super::policy::{EVENT_COMPACTED, EVENT_FAILED};
use crate::telemetry::context::{CompactionMark, Reading};
use crate::telemetry::readers::Unreadable;
use crate::workspace::model::at_prompt;

/// Everything the job does to the world.
pub trait JobPorts {
    /// The pane's agent status (`idle`, `done`, `working`, ...). None: no
    /// status.
    fn pane_status(&self) -> Result<Option<String>>;
    /// Type 1 line into the target pane.
    fn send(&self, line: &str) -> Result<()>;
    /// The target session's current context.
    fn reading(&self) -> Result<Reading, Unreadable>;
    /// Record a ledger event on the target record.
    fn record_event(&self, event: &str, text: &str) -> Result<()>;
    /// Send 1 untagged line to the orchestrator pane.
    fn report(&self, line: &str) -> Result<()>;
    /// Append 1 line to `job.log`.
    fn log(&self, step: Step, text: &str);
    fn sleep(&self, d: Duration);
    /// Time since the job started (the test clock in tests).
    fn elapsed(&self) -> Duration;
}

/// What 1 job does. The CLI renders every placeholder of the templates
/// except `{pre}`, `{post}`, `{step}` and `{reason}`, which the job fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPlan {
    pub compact_line: String,
    pub max_sends: u8,
    pub resume_line: String,
    pub reported_tmpl: String,
    pub failed_tmpl: String,
    /// The handoff path, as the `compacted` event names it.
    pub handoff: String,
    pub role: String,
    pub log_path: String,
    /// Between 2 polls: 2 s.
    pub poll: Duration,
    /// Idle polls in a row that count as idle: 2.
    pub idle_polls: u8,
    /// The wait for idle before a send (`--timeout`, default 1800 s).
    pub idle_timeout: Duration,
    /// With more sends allowed: send again after this with no marker (30 s).
    pub resend_after: Duration,
    /// The wait for the marker after the first send (600 s).
    pub marker_timeout: Duration,
    /// The wait for idle after the marker (600 s).
    pub settle_timeout: Duration,
    /// The wait for a reading that is not pending (180 s).
    pub reading_timeout: Duration,
}

/// The steps of the job. `Start`: the job did not start, or the child
/// refused before its first job step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    WaitIdle,
    Send,
    WaitMarker,
    WaitIdleAfter,
    Resume,
    WaitReading,
    Report,
}

impl Step {
    /// The kebab-case name in `job.json`, the log and the event text.
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Start => "start",
            Step::WaitIdle => "wait-idle",
            Step::Send => "send",
            Step::WaitMarker => "wait-marker",
            Step::WaitIdleAfter => "wait-idle-after",
            Step::Resume => "resume",
            Step::WaitReading => "wait-reading",
            Step::Report => "report",
        }
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why the job stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFailure {
    pub step: Step,
    pub reason: String,
}

/// A confirmed compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOutcome {
    pub pre: Option<u64>,
    pub post: Option<u64>,
    pub mark: CompactionMark,
}

/// Run the job. On failure the event and the report are already sent.
pub fn run(ports: &dyn JobPorts, plan: &JobPlan) -> Result<JobOutcome, JobFailure> {
    let mut pre = None;
    let result = steps(ports, plan, &mut pre);
    match &result {
        Ok(done) => {
            ports.log(Step::Report, "compacted");
            let text = format!(
                "{} -> {} tokens; handoff {}",
                number(done.pre),
                number(done.post),
                plan.handoff
            );
            best_effort(
                ports,
                Step::Report,
                ports.record_event(EVENT_COMPACTED, &text),
            );
            let line = fill(&plan.reported_tmpl, done.pre, done.post, Step::Report, "");
            best_effort(ports, Step::Report, ports.report(&line));
        }
        Err(fail) => {
            ports.log(fail.step, &format!("failed: {}", fail.reason));
            let text = format!("step {}: {}", fail.step, fail.reason);
            best_effort(ports, fail.step, ports.record_event(EVENT_FAILED, &text));
            let line = fill(&plan.failed_tmpl, pre, None, fail.step, &fail.reason);
            best_effort(ports, fail.step, ports.report(&line));
        }
    }
    result
}

fn steps(
    ports: &dyn JobPorts,
    plan: &JobPlan,
    pre_out: &mut Option<u64>,
) -> Result<JobOutcome, JobFailure> {
    let first = ports.reading().map_err(|e| JobFailure {
        step: Step::Start,
        reason: format!("no reading of the session: {e}"),
    })?;
    let pre = first.tokens;
    *pre_out = pre;
    // A marker at or after `start` is new. Every marker already in the
    // file is at or before the newest one, so 1 past it is enough, and the
    // job reads no file itself.
    let start = first.last_compaction.as_ref().map_or(0, |m| m.offset + 1);

    let max_sends = plan.max_sends.max(1);
    let mut sends = 0u8;
    let mut marker_since = None;
    let mark = 'send: loop {
        wait_idle(ports, plan, Step::WaitIdle, plan.idle_timeout)?;
        ports.log(Step::Send, &plan.compact_line);
        ports.send(&plan.compact_line).map_err(|e| JobFailure {
            step: Step::Send,
            reason: format!("{e:#}"),
        })?;
        sends += 1;
        let since = ports.elapsed();
        let first_send = *marker_since.get_or_insert(since);
        ports.log(Step::WaitMarker, &format!("send {sends} of {max_sends}"));
        loop {
            if let Ok(r) = ports.reading() {
                if let Some(m) = r.last_compaction.filter(|m| m.offset >= start) {
                    break 'send m;
                }
            }
            let now = ports.elapsed();
            if now.saturating_sub(first_send) >= plan.marker_timeout {
                return Err(JobFailure {
                    step: Step::WaitMarker,
                    reason: format!("no compaction marker after {sends} sends"),
                });
            }
            if sends < max_sends && now.saturating_sub(since) >= plan.resend_after {
                continue 'send;
            }
            ports.sleep(plan.poll);
        }
    };
    ports.log(Step::WaitMarker, &format!("marker at byte {}", mark.offset));

    wait_idle(ports, plan, Step::WaitIdleAfter, plan.settle_timeout)?;
    ports.log(Step::Resume, &plan.resume_line);
    ports.send(&plan.resume_line).map_err(|e| JobFailure {
        step: Step::Resume,
        reason: format!("{e:#}"),
    })?;

    ports.log(Step::WaitReading, "waiting for a response after the marker");
    let begin = ports.elapsed();
    let mut post = None;
    loop {
        if let Ok(r) = ports.reading() {
            if !r.pending {
                post = r.tokens;
                break;
            }
        }
        if ports.elapsed().saturating_sub(begin) >= plan.reading_timeout {
            break;
        }
        ports.sleep(plan.poll);
    }
    Ok(JobOutcome {
        pre,
        post: post.or(mark.post_tokens),
        mark,
    })
}

/// Wait for `idle_polls` polls in a row whose status is at the prompt
/// ([`at_prompt`]: `idle` or `done`). Any other status, none, or a failed
/// poll starts the count again.
fn wait_idle(
    ports: &dyn JobPorts,
    plan: &JobPlan,
    step: Step,
    timeout: Duration,
) -> Result<(), JobFailure> {
    ports.log(step, "waiting for idle");
    let begin = ports.elapsed();
    let mut idle = 0u8;
    loop {
        let ready = match ports.pane_status() {
            Ok(status) => at_prompt(status.as_deref()),
            Err(_) => false,
        };
        idle = if ready { idle + 1 } else { 0 };
        if idle >= plan.idle_polls.max(1) {
            return Ok(());
        }
        if ports.elapsed().saturating_sub(begin) >= timeout {
            return Err(JobFailure {
                step,
                reason: format!("no idle status in {} s", timeout.as_secs()),
            });
        }
        ports.sleep(plan.poll);
    }
}

fn best_effort(ports: &dyn JobPorts, step: Step, result: Result<()>) {
    if let Err(e) = result {
        ports.log(step, &format!("not sent: {e:#}"));
    }
}

/// A token count, or `unknown`.
fn number(n: Option<u64>) -> String {
    n.map_or_else(|| "unknown".into(), |n| n.to_string())
}

/// Fill the 4 tokens the job knows. Every other placeholder was rendered
/// by the CLI.
fn fill(tmpl: &str, pre: Option<u64>, post: Option<u64>, step: Step, reason: &str) -> String {
    tmpl.replace("{pre}", &number(pre))
        .replace("{post}", &number(post))
        .replace("{step}", step.as_str())
        .replace("{reason}", reason)
}
