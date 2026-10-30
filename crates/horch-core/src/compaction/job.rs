//! The compaction job's state machine (CTX-13, CTX-15, CTX-22, CTX-23).
//!
//! [`run`] waits for the pane to be idle, types the compact line, confirms
//! the compaction by the transcript marker, waits for idle again, puts a
//! switched pane back in its launch mode, types the resume line, and
//! reports. A failure line that the harness prints in the
//! pane after the send stops the job at once. Every effect goes through [`JobPorts`], so a
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
    /// The target pane's recent output, 1 terminal line per text line.
    fn pane_text(&self) -> Result<String>;
    /// The target session's current context.
    fn reading(&self) -> Result<Reading, Unreadable>;
    /// Record a ledger event on the target record.
    fn record_event(&self, event: &str, text: &str) -> Result<()>;
    /// Send 1 untagged line to the orchestrator pane.
    fn report(&self, line: &str) -> Result<()>;
    /// Put the pane back in its launch mode before the resume line: a Codex
    /// pane with `permission_mode: plan` that `--request` switched to write
    /// (X6). `None`: nothing to restore.
    fn restore_mode(&self) -> Option<Result<()>> {
        None
    }
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
    /// The harness name in a failure reason (`pi said "..."`).
    pub harness: String,
    /// The harness's compaction failure lines, as substrings
    /// ([`CompactionCaps::failure_lines`](crate::harness::capabilities::CompactionCaps::failure_lines)).
    pub failure_lines: Vec<String>,
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
    RestoreMode,
    Resume,
    WaitReading,
    Report,
}

/// [`Step::as_str`] by variant order. The `step_names_are_kebab_case_in_order`
/// test pins it.
const STEP_NAMES: [&str; 9] = [
    "start",
    "wait-idle",
    "send",
    "wait-marker",
    "wait-idle-after",
    "restore-mode",
    "resume",
    "wait-reading",
    "report",
];

impl Step {
    /// The kebab-case name in `job.json`, the log and the event text.
    pub fn as_str(self) -> &'static str {
        STEP_NAMES[self as usize]
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
    let first = ports
        .reading()
        .map_err(|e| failed(Step::Start, format!("no reading of the session: {e}")))?;
    let pre = first.tokens;
    *pre_out = pre;
    // A marker at or after `start` is new. Every marker already in the
    // file is at or before the newest one, so 1 past it is enough, and the
    // job reads no file itself.
    let start = first.last_compaction.as_ref().map_or(0, |m| m.offset + 1);

    let mark = send_until_marker(ports, plan, start)?;
    ports.log(Step::WaitMarker, &format!("marker at byte {}", mark.offset));
    resume(ports, plan)?;
    let post = wait_post_tokens(ports, plan);
    Ok(JobOutcome {
        pre,
        post: post.or(mark.post_tokens),
        mark,
    })
}

fn failed(step: Step, reason: String) -> JobFailure {
    JobFailure { step, reason }
}

/// One send of the compact line, and what the wait for its marker needs.
struct Sent {
    /// The pane text before the send.
    before: Option<String>,
    /// The first byte offset of a new marker.
    start: u64,
    /// When the first send happened.
    first: Duration,
    /// When this send happened.
    at: Duration,
    /// This send's number, from 1.
    n: u8,
    max: u8,
}

/// What 1 poll of the wait for the marker found.
enum Poll {
    Marked(CompactionMark),
    Failed(JobFailure),
    /// No marker after `resend_after`, and more sends are allowed.
    Resend,
    Wait,
}

/// Send the compact line, and send it again when no marker comes within
/// `resend_after` and more sends are allowed.
fn send_until_marker(
    ports: &dyn JobPorts,
    plan: &JobPlan,
    start: u64,
) -> Result<CompactionMark, JobFailure> {
    let max = plan.max_sends.max(1);
    let mut first = None;
    let mut n = 0u8;
    loop {
        n += 1;
        wait_idle(ports, plan, Step::WaitIdle, plan.idle_timeout)?;
        let before = pane_before(ports, plan);
        ports.log(Step::Send, &plan.compact_line);
        ports
            .send(&plan.compact_line)
            .map_err(|e| failed(Step::Send, format!("{e:#}")))?;
        let at = ports.elapsed();
        let first = *first.get_or_insert(at);
        ports.log(Step::WaitMarker, &format!("send {n} of {max}"));
        let sent = Sent {
            before,
            start,
            first,
            at,
            n,
            max,
        };
        match await_marker(ports, plan, &sent) {
            Poll::Marked(mark) => break Ok(mark),
            Poll::Failed(fail) => break Err(fail),
            Poll::Resend | Poll::Wait => {}
        }
    }
}

/// Poll until a marker, a failure, or the time for a resend.
fn await_marker(ports: &dyn JobPorts, plan: &JobPlan, sent: &Sent) -> Poll {
    loop {
        let poll = poll_marker(ports, plan, sent);
        if !matches!(poll, Poll::Wait) {
            break poll;
        }
        ports.sleep(plan.poll);
    }
}

fn poll_marker(ports: &dyn JobPorts, plan: &JobPlan, sent: &Sent) -> Poll {
    let marked = ports
        .reading()
        .ok()
        .and_then(|r| r.last_compaction)
        .filter(|m| m.offset >= sent.start);
    if let Some(mark) = marked {
        Poll::Marked(mark)
    } else if let Some(line) = sent
        .before
        .as_ref()
        .and_then(|b| new_failure(ports, plan, b))
    {
        let reason = format!("{} said \"{line}\"", plan.harness);
        Poll::Failed(failed(Step::WaitMarker, reason))
    } else {
        poll_clock(ports, plan, sent)
    }
}

/// The poll of a wait with no marker and no failure line: the timeout, the
/// resend, or on.
fn poll_clock(ports: &dyn JobPorts, plan: &JobPlan, sent: &Sent) -> Poll {
    let now = ports.elapsed();
    if now.saturating_sub(sent.first) >= plan.marker_timeout {
        let reason = format!("no compaction marker after {} sends", sent.n);
        Poll::Failed(failed(Step::WaitMarker, reason))
    } else if sent.n < sent.max && now.saturating_sub(sent.at) >= plan.resend_after {
        Poll::Resend
    } else {
        Poll::Wait
    }
}

/// Wait for idle, put a switched pane back in its launch mode, and type the
/// resume line.
fn resume(ports: &dyn JobPorts, plan: &JobPlan) -> Result<(), JobFailure> {
    wait_idle(ports, plan, Step::WaitIdleAfter, plan.settle_timeout)?;
    // Before the resume line: the switch needs the pane at its prompt.
    if let Some(restored) = ports.restore_mode() {
        ports.log(Step::RestoreMode, "back to the launch mode");
        restored.map_err(|e| failed(Step::RestoreMode, format!("{e:#}")))?;
    }
    ports.log(Step::Resume, &plan.resume_line);
    ports
        .send(&plan.resume_line)
        .map_err(|e| failed(Step::Resume, format!("{e:#}")))
}

/// The token count of the first reading that is not pending, or None at
/// `reading_timeout`.
fn wait_post_tokens(ports: &dyn JobPorts, plan: &JobPlan) -> Option<u64> {
    ports.log(Step::WaitReading, "waiting for a response after the marker");
    let begin = ports.elapsed();
    loop {
        if let Some(reading) = ports.reading().ok().filter(|r| !r.pending) {
            break reading.tokens;
        }
        if ports.elapsed().saturating_sub(begin) >= plan.reading_timeout {
            break None;
        }
        ports.sleep(plan.poll);
    }
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
        let ready = ports.pane_status().is_ok_and(|s| at_prompt(s.as_deref()));
        idle = if ready { idle + 1 } else { 0 };
        if idle >= plan.idle_polls.max(1) {
            break Ok(());
        }
        if ports.elapsed().saturating_sub(begin) >= timeout {
            break Err(failed(
                step,
                format!("no idle status in {} s", timeout.as_secs()),
            ));
        }
        ports.sleep(plan.poll);
    }
}

/// The pane text right before a send, so that only lines after the send
/// count. None: no failure lines, or the pane did not read (then the job
/// waits for the marker only).
fn pane_before(ports: &dyn JobPorts, plan: &JobPlan) -> Option<String> {
    if plan.failure_lines.is_empty() {
        return None;
    }
    match ports.pane_text() {
        Ok(text) => Some(text),
        Err(e) => {
            ports.log(
                Step::Send,
                &format!("no pane text, no failure check: {e:#}"),
            );
            None
        }
    }
}

/// The first failure line that the pane shows more often now than in
/// `before`. A count, not a position: a TUI redraws its screen, and old
/// lines leave the scrollback, so neither makes an old line new.
fn new_failure(ports: &dyn JobPorts, plan: &JobPlan, before: &str) -> Option<String> {
    let now = ports.pane_text().ok()?;
    let failure = |l: &&str| plan.failure_lines.iter().any(|f| l.contains(f.as_str()));
    let count = |text: &str, line: &str| text.lines().filter(|l| l.trim() == line).count();
    now.lines()
        .filter(failure)
        .map(str::trim)
        .find(|line| count(&now, line) > count(before, line))
        .map(|line| line.chars().take(MAX_LINE_CHARS).collect())
}

/// The longest harness line that a report quotes.
const MAX_LINE_CHARS: usize = 200;

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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    use super::*;

    /// A pane that is always idle and compacts on the first send. Every
    /// effect is 1 entry in `trail`.
    struct Fake {
        trail: RefCell<Vec<String>>,
        sent: Cell<u8>,
        clock: Cell<Duration>,
        restore: Option<Result<(), String>>,
        /// The marker shows from this send on; 0: never.
        marker_on_send: u8,
        idle: bool,
        pending: bool,
        tokens_after: u64,
        /// The pane text per `pane_text` call; the last one repeats.
        texts: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(restore: Option<Result<(), String>>) -> Fake {
            Fake {
                trail: RefCell::new(vec![]),
                sent: Cell::new(0),
                clock: Cell::new(Duration::ZERO),
                restore,
                marker_on_send: 1,
                idle: true,
                pending: false,
                tokens_after: 5,
                texts: RefCell::new(vec![String::new()]),
            }
        }

        fn trail(&self) -> Vec<String> {
            self.trail.borrow().clone()
        }
    }

    impl JobPorts for Fake {
        fn pane_status(&self) -> Result<Option<String>> {
            Ok(Some(if self.idle { "idle" } else { "working" }.into()))
        }
        fn send(&self, line: &str) -> Result<()> {
            self.trail.borrow_mut().push(format!("send {line}"));
            self.sent.set(self.sent.get() + 1);
            Ok(())
        }
        fn pane_text(&self) -> Result<String> {
            let mut texts = self.texts.borrow_mut();
            Ok(if texts.len() > 1 {
                texts.remove(0)
            } else {
                texts[0].clone()
            })
        }
        fn reading(&self) -> Result<Reading, Unreadable> {
            let sent = self.sent.get();
            let marked = self.marker_on_send > 0 && sent >= self.marker_on_send;
            let mark = marked.then(|| CompactionMark {
                at: "2026-10-07T00:00:00Z".into(),
                offset: 10,
                trigger: None,
                pre_tokens: None,
                post_tokens: Some(5),
            });
            Ok(Reading {
                tokens: Some(if mark.is_some() {
                    self.tokens_after
                } else {
                    100
                }),
                provisional: false,
                pending: self.pending && mark.is_some(),
                window: None,
                model: None,
                last_compaction: mark,
                marks: 0,
                marks_at: vec![],
                transcript: PathBuf::new(),
            })
        }
        fn record_event(&self, event: &str, text: &str) -> Result<()> {
            self.trail
                .borrow_mut()
                .push(format!("event {event} {text}"));
            Ok(())
        }
        fn report(&self, line: &str) -> Result<()> {
            self.trail.borrow_mut().push(format!("report {line}"));
            Ok(())
        }
        fn restore_mode(&self) -> Option<Result<()>> {
            let r = self.restore.clone()?;
            self.trail.borrow_mut().push("restore-mode".into());
            Some(r.map_err(|e| anyhow::anyhow!(e)))
        }
        fn log(&self, _: Step, _: &str) {}
        fn sleep(&self, d: Duration) {
            self.clock.set(self.clock.get() + d);
        }
        fn elapsed(&self) -> Duration {
            self.clock.get()
        }
    }

    fn plan() -> JobPlan {
        JobPlan {
            compact_line: "/compact".into(),
            max_sends: 1,
            resume_line: "resume".into(),
            reported_tmpl: "ok {pre} -> {post}".into(),
            failed_tmpl: "failed at {step}: {reason}".into(),
            handoff: "h.md".into(),
            role: "codex-plan-1".into(),
            harness: "codex".into(),
            failure_lines: vec![],
            log_path: "job.log".into(),
            poll: Duration::from_secs(1),
            idle_polls: 2,
            idle_timeout: Duration::from_secs(10),
            resend_after: Duration::from_secs(30),
            marker_timeout: Duration::from_secs(60),
            settle_timeout: Duration::from_secs(10),
            reading_timeout: Duration::from_secs(10),
        }
    }

    /// X6: a plan pane goes back to read-only after the marker and idle,
    /// before the resume line, so the worker resumes read-only.
    #[test]
    fn a_plan_pane_is_restored_before_the_resume_line() {
        let fake = Fake::new(Some(Ok(())));
        assert!(run(&fake, &plan()).is_ok());
        assert_eq!(
            fake.trail()[..3],
            ["send /compact", "restore-mode", "send resume"]
        );
    }

    /// A failed restore fails the job at `restore-mode`: 1 event, 1
    /// report, and no resume line.
    #[test]
    fn a_failed_restore_stops_the_job_before_the_resume_line() {
        let fake = Fake::new(Some(Err("the menu did not open".into())));
        let fail = run(&fake, &plan()).unwrap_err();
        assert_eq!(fail.step, Step::RestoreMode);
        assert_eq!(
            fake.trail(),
            [
                "send /compact",
                "restore-mode",
                "event compact-failed step restore-mode: the menu did not open",
                "report failed at restore-mode: the menu did not open",
            ]
        );
    }

    /// Without a mode to restore, the job goes from the marker to the
    /// resume line.
    #[test]
    fn no_restore_without_a_mode_to_restore() {
        let fake = Fake::new(None);
        assert!(run(&fake, &plan()).is_ok());
        assert_eq!(fake.trail()[..2], ["send /compact", "send resume"]);
    }

    /// With more sends allowed, a send with no marker is repeated after
    /// `resend_after`, and the marker of the second send ends the wait.
    #[test]
    fn a_missing_marker_sends_the_line_again_after_the_resend_wait() {
        let mut fake = Fake::new(None);
        fake.marker_on_send = 2;
        let mut p = plan();
        p.max_sends = 2;
        assert!(run(&fake, &p).is_ok());
        assert_eq!(
            fake.trail()[..3],
            ["send /compact", "send /compact", "send resume"]
        );
    }

    /// With 1 send allowed, no marker fails the job at `wait-marker`.
    #[test]
    fn no_marker_fails_the_job_at_wait_marker_after_the_timeout() {
        let mut fake = Fake::new(None);
        fake.marker_on_send = 0;
        let fail = run(&fake, &plan()).unwrap_err();
        assert_eq!(fail.step, Step::WaitMarker);
        assert_eq!(fail.reason, "no compaction marker after 1 sends");
        assert_eq!(
            fake.trail()
                .iter()
                .filter(|t| *t == "send /compact")
                .count(),
            1
        );
        // 1 s to idle (2 polls), then the 60 s marker wait.
        assert_eq!(fake.elapsed(), Duration::from_secs(61));
    }

    /// A failure line that is new after the send stops the job at once and
    /// names the harness.
    #[test]
    fn a_new_failure_line_stops_the_wait_for_the_marker() {
        let mut fake = Fake::new(None);
        fake.marker_on_send = 0;
        *fake.texts.borrow_mut() = vec!["ready".into(), "ready\n  boom: no room  ".into()];
        let mut p = plan();
        p.failure_lines = vec!["boom".into()];
        let fail = run(&fake, &p).unwrap_err();
        assert_eq!(fail.step, Step::WaitMarker);
        assert_eq!(fail.reason, "codex said \"boom: no room\"");
        // Only the 1 s to idle: the first marker poll sees the failure.
        assert_eq!(fake.elapsed(), Duration::from_secs(1));
    }

    /// A failure line that was in the pane before the send is not new.
    #[test]
    fn an_old_failure_line_does_not_stop_the_job() {
        let fake = Fake::new(None);
        *fake.texts.borrow_mut() = vec!["boom".into()];
        let mut p = plan();
        p.failure_lines = vec!["boom".into()];
        assert!(run(&fake, &p).is_ok());
    }

    #[test]
    fn a_pane_that_never_idles_fails_the_job_at_wait_idle() {
        let mut fake = Fake::new(None);
        fake.idle = false;
        let fail = run(&fake, &plan()).unwrap_err();
        assert_eq!(fail.step, Step::WaitIdle);
        assert_eq!(fail.reason, "no idle status in 10 s");
        assert!(fake.trail().iter().all(|t| !t.starts_with("send")));
    }

    #[test]
    fn the_outcome_carries_the_reading_after_the_resume_line() {
        let mut fake = Fake::new(None);
        fake.tokens_after = 7;
        let done = run(&fake, &plan()).unwrap();
        assert_eq!((done.pre, done.post), (Some(100), Some(7)));
        assert_eq!(done.mark.offset, 10);
        assert_eq!(fake.trail().last().unwrap(), "report ok 100 -> 7");
    }

    /// A reading that stays pending past `reading_timeout` falls back to the
    /// marker's own post count.
    #[test]
    fn a_pending_reading_falls_back_to_the_marker_post_tokens() {
        let mut fake = Fake::new(None);
        fake.pending = true;
        fake.tokens_after = 7;
        let done = run(&fake, &plan()).unwrap();
        assert_eq!(done.post, Some(5));
        // 1 s to idle before the send, 1 s after the marker, 10 s reading wait.
        assert_eq!(fake.elapsed(), Duration::from_secs(12));
    }

    #[test]
    fn step_names_are_kebab_case_in_order() {
        let all = [
            Step::Start,
            Step::WaitIdle,
            Step::Send,
            Step::WaitMarker,
            Step::WaitIdleAfter,
            Step::RestoreMode,
            Step::Resume,
            Step::WaitReading,
            Step::Report,
        ];
        let names: Vec<_> = all.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            names,
            [
                "start",
                "wait-idle",
                "send",
                "wait-marker",
                "wait-idle-after",
                "restore-mode",
                "resume",
                "wait-reading",
                "report"
            ]
        );
    }
}
