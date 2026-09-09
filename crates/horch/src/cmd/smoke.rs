//! `horch smoke` - the port of the `horch1-smoke` and `herdr-fleet-smoke` recipes.
//!
//! These are the acceptance tests for the machinery. They spend no LLM tokens and
//! clean up after themselves on success, leaving the scratch workspace open for
//! inspection on failure.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use horch_core::herdr::{Direction, Herdr};
use horch_core::ledger::Ledger;
use horch_core::mailbox::Mailbox;
use horch_core::paneshell::PaneShell;

use super::doctor;
use super::spawn::{spawn, SpawnArgs};

#[derive(Subcommand)]
pub enum SmokeCommand {
    /// Two-pane check of the herdr messaging primitives, with no agents involved.
    /// Run this first against a new herdr install.
    Messaging,
    /// End-to-end check of the fleet machinery using a token-free fake agent.
    Fleet,
}

pub fn run(command: SmokeCommand) -> Result<ExitCode> {
    match command {
        SmokeCommand::Messaging => messaging(),
        SmokeCommand::Fleet => fleet(),
    }
}

/// Poll `check` every `interval` until it returns true or `attempts` run out.
fn poll(attempts: u32, interval: Duration, mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..attempts {
        if check() {
            return true;
        }
        std::thread::sleep(interval);
    }
    false
}

/// A shell command that prints `SMOKE_TEST_42` only when the pane's shell actually
/// evaluates it.
///
/// Matching the arithmetic *result* rather than the sent text is the point:
/// matching the text alone would false-pass on an input line that was pasted but
/// never submitted, which is exactly the failure `horch tell` works around.
fn arithmetic_echo(shell: PaneShell) -> &'static str {
    match shell {
        PaneShell::Posix => "echo SMOKE_TEST_$((40+2))",
        PaneShell::PowerShell => "Write-Output \"SMOKE_TEST_$(40+2)\"",
    }
}

/// Cheap, self-verifying 2-pane check of the herdr messaging primitives
/// (send-text + send-keys enter, and the mailbox registry).
fn messaging() -> Result<ExitCode> {
    doctor::check()?;
    let herdr = Herdr::new();
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let shell = PaneShell::host();

    let ws = herdr.workspace_create("horch smoke test", None, false)?;
    let a = ws.root_pane_id.clone();
    let b = herdr.pane_split(&a, Direction::Right)?;

    for (pane, role) in [(&a, "a"), (&b, "b")] {
        herdr.pane_run(pane, &shell.command_line(&exe, &["register", role]))?;
    }

    // Registration shells out to `herdr pane get`; poll the mailbox rather than
    // guessing a sleep.
    let mailbox = Mailbox::new(&ws.workspace_id);
    let registered = poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("a").is_some() && mailbox.pane_for("b").is_some()
    });
    if !registered {
        eprintln!(
            "FAIL: panes never registered (no {}/{{a,b}}.id). \
             Leaving workspace {} open for inspection.",
            mailbox.dir().display(),
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!("Sending a test message a -> b via horch tell...");
    let target = mailbox.pane_for("b").expect("just polled");
    herdr.send_line(&target, arithmetic_echo(shell))?;

    if herdr.wait_output(&b, "SMOKE_TEST_42", 15_000)? {
        println!("PASS: send-text + send-keys enter delivered and executed the message end to end.");
        herdr.workspace_close(&ws.workspace_id)?;
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!(
        "FAIL: SMOKE_TEST_42 not observed on pane {b}. \
         Leaving workspace {} open for inspection.",
        ws.workspace_id
    );
    eprintln!("--- pane {b} contents ---");
    match herdr.pane_read(&b, "recent-unwrapped") {
        Ok(contents) => eprintln!("{contents}"),
        Err(e) => eprintln!("(could not read pane: {e:#})"),
    }
    Ok(ExitCode::FAILURE)
}

/// Self-verifying check of the fleet machinery (spawn -> brief -> register ->
/// ledger add/note/done -> tell -> pane self-close) using a token-free fake agent
/// in a scratch workspace with an isolated ledger.
fn fleet() -> Result<ExitCode> {
    doctor::check()?;
    let herdr = Herdr::new();
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let shell = PaneShell::host();

    // Isolated state so the check never touches a real project ledger.
    let state_dir = tempfile::Builder::new().prefix("horch-smoke-state").tempdir()?;
    let project_dir = tempfile::Builder::new().prefix("horch-smoke-proj").tempdir()?;
    let project = project_dir.path().to_string_lossy().into_owned();
    std::env::set_var("HORCH_STATE_DIR", state_dir.path());
    std::env::set_var("HORCH_PROJECT_DIR", &project);

    let ws = herdr.workspace_create("herdr-fleet smoke", Some(&project), false)?;
    std::env::set_var("HORCH_WORKSPACE_ID", &ws.workspace_id);
    let mailbox = Mailbox::new(&ws.workspace_id);

    // A fake orchestrator (no agent) so the worker's `horch done` has a
    // `horch tell` target to report DONE to.
    herdr.pane_run(
        &ws.root_pane_id,
        &shell.command_line(&exe, &["register", "orchestrator"]),
    )?;
    if !poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("orchestrator").is_some()
    }) {
        eprintln!(
            "FAIL: orchestrator pane never registered. Workspace {} left open.",
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!("Spawning smoke worker...");
    let pane = spawn(SpawnArgs {
        teammate: Some("smoke".to_string()),
        task: "verify fleet machinery".to_string(),
        resume: None,
        role: None,
        from_pane: Some(ws.root_pane_id.clone()),
        direction: Direction::Right,
    })?;

    let ledger = Ledger::open()?;
    let reached_done = poll(30, Duration::from_secs(1), || smoke_session_done(&ledger));
    if !reached_done {
        eprintln!(
            "FAIL: smoke session never reached done in the ledger. Workspace {} left open.",
            ws.workspace_id
        );
        dump_ledger(ledger.path());
        return Ok(ExitCode::FAILURE);
    }

    // The worker must have closed its own pane.
    if !poll(10, Duration::from_secs(1), || !herdr.pane_exists(&pane)) {
        eprintln!(
            "FAIL: smoke worker did not close its own pane {pane}. Workspace {} left open.",
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!(
        "PASS: spawn, brief, register, ledger add/note/done, tell, and pane self-close \
         all verified."
    );
    herdr.workspace_close(&ws.workspace_id)?;
    Ok(ExitCode::SUCCESS)
}

/// Exactly one session, finished, having recorded both a progress note and a
/// completion summary.
fn smoke_session_done(ledger: &Ledger) -> bool {
    let Ok(records) = ledger.read() else {
        return false;
    };
    let [record] = records.as_slice() else {
        return false;
    };
    let events: Vec<&str> = record.history.iter().map(|h| h.event.as_str()).collect();
    record.status == horch_core::ledger::STATUS_DONE
        && events.contains(&"note")
        && events.contains(&"done")
}

fn dump_ledger(path: &Path) {
    eprintln!("--- ledger ({}) ---", path.display());
    match std::fs::read_to_string(path) {
        Ok(contents) => eprintln!("{contents}"),
        Err(e) => eprintln!("(could not read ledger: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic must be left for the pane's own shell to evaluate, in that
    /// shell's dialect - never pre-computed here.
    #[test]
    fn arithmetic_echo_is_unevaluated_and_shell_appropriate() {
        for shell in [PaneShell::Posix, PaneShell::PowerShell] {
            let cmd = arithmetic_echo(shell);
            assert!(!cmd.contains("SMOKE_TEST_42"), "{cmd} must not be pre-evaluated");
            assert!(cmd.contains("40+2"), "{cmd}");
        }
        assert!(arithmetic_echo(PaneShell::Posix).starts_with("echo "));
        assert!(arithmetic_echo(PaneShell::PowerShell).starts_with("Write-Output "));
    }

    #[test]
    fn poll_stops_at_the_first_success() {
        let mut calls = 0;
        assert!(poll(5, Duration::ZERO, || {
            calls += 1;
            calls == 2
        }));
        assert_eq!(calls, 2);
    }

    #[test]
    fn poll_gives_up_after_the_attempt_budget() {
        let mut calls = 0;
        assert!(!poll(3, Duration::ZERO, || {
            calls += 1;
            false
        }));
        assert_eq!(calls, 3);
    }

    #[test]
    fn smoke_session_is_done_only_with_one_finished_noted_record() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/p");
        assert!(!smoke_session_done(&ledger), "empty ledger");

        ledger
            .add("r1", "none", "smoke", "none", "smoke-1", Some("s1"), "t")
            .unwrap();
        assert!(!smoke_session_done(&ledger), "still working");

        ledger.note("r1", "progress").unwrap();
        assert!(!smoke_session_done(&ledger), "noted but not done");

        ledger.done("r1", "finished").unwrap();
        assert!(smoke_session_done(&ledger));

        // A second session means something else spawned into the scratch ledger.
        ledger
            .add("r2", "none", "smoke", "none", "smoke-2", Some("s2"), "t")
            .unwrap();
        assert!(!smoke_session_done(&ledger));
    }

    /// A session that finished without ever recording a note means `horch note`
    /// silently failed, which the check must catch.
    #[test]
    fn a_done_session_without_a_note_fails_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/p");
        ledger
            .add("r1", "none", "smoke", "none", "smoke-1", Some("s1"), "t")
            .unwrap();
        ledger.done("r1", "finished").unwrap();
        assert!(!smoke_session_done(&ledger));
    }
}
