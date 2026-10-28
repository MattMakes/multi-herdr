//! End-to-end (G5): fleet messaging through the real `horch` binary, in a
//! workspace with no registered orchestrator.
//!
//! - `horch done` records the summary, tells nobody, says why, exits 0.
//! - `horch inbox` marks a role whose pane closed without `horch done`.
//! - W2: a `horch done` whose report fails keeps the pane, the role and the
//!   live record, exits non-zero, and a retry finishes once.
//! - W2: an agent that exits before `horch done` leaves an `exited` event,
//!   and its worker tells the orchestrator with a `BLOCKED:` line.
//! - `horch spawn --resume` of that record ends it and resumes it; while the
//!   pane is open, it refuses and names the command that fixes it.
//!
//! fake-herdr runs in the `exec` scenario, so a pane runs `horch worker`, and
//! `pane close` kills it. The codex fake runs in `stay`.

use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

const NOW: &str = "2026-09-28T18:00:00Z";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with workspace `w1` and its root pane `w1:p1`, as `horch fleet`
/// leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NOW", NOW);
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let out = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    h
}

fn records(h: &Harness) -> Vec<Value> {
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    std::fs::read_to_string(h.state.join(format!("{slug}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn record(h: &Harness, id: &str) -> Value {
    records(h)
        .into_iter()
        .find(|r| r["record_id"] == id)
        .unwrap_or_else(|| panic!("no record {id}"))
}

/// What a worker pane wrote (fake-herdr `exec` saves its output), and the
/// record, for a failure message.
fn pane_context(h: &Harness, pane: &str, id: &str) -> String {
    let mut out = h.log.clone().into_os_string();
    out.push(format!(".pane-{}.out", pane.replace(':', "_")));
    format!(
        "record: {}\npane output:\n{}",
        record(h, id),
        std::fs::read_to_string(&out).unwrap_or_else(|e| format!("({e})"))
    )
}

/// `horch spawn`, returning the new pane id (the last stdout line).
fn spawn(h: &Harness, args: &[&str]) -> String {
    let mut argv = vec!["spawn"];
    argv.extend_from_slice(args);
    argv.extend_from_slice(&["--from-pane", "w1:p1", "--no-tile"]);
    let out = h.run(&argv);
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string()
}

/// `horch done` as the agent in `pane` runs it: the transport variables say
/// who it is. Closing the pane kills the worker and its agent.
fn done(h: &Harness, pane: &str, role: &str, id: &str) {
    let mut cmd = h.horch(&["done", "finished the work"]);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

/// A fake-herdr call, as the operator would make it by hand.
fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

/// `horch` as the worker in `pane` runs it.
fn as_worker(h: &Harness, args: &[&str], pane: &str, role: &str, id: &str) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    cmd.output().unwrap()
}

/// A spawned codex worker that is running, as `(pane, role, record id)`.
fn running_worker(h: &Harness) -> (String, String, String) {
    let pane = spawn(h, &["codex-sol", "build the thing"]);
    let r = records(h).pop().expect("the spawn wrote a record");
    let id = r["record_id"].as_str().unwrap().to_string();
    let role = r["role"].as_str().unwrap().to_string();
    wait_for_or(
        "the running worker",
        || {
            let r = record(h, &id);
            (r["state"]["state"] == "running" && r["session_id"].is_string()).then_some(())
        },
        || pane_context(h, &pane, &id),
    );
    (pane, role, id)
}

/// G5: in a workspace with no registered orchestrator, a worker's `tell` to
/// it fails and says that `horch done` still works; `done` records the
/// summary, tells nobody, says why, and exits 0.
#[test]
fn g5_done_without_an_orchestrator() {
    let h = fleet("g5-done", "exec,stay");
    let (pane, role, id) = running_worker(&h);

    let tell = as_worker(&h, &["tell", "orchestrator", "DONE: x"], &pane, &role, &id);
    assert!(!tell.status.success(), "{}", text(&tell));
    assert!(
        text(&tell).contains("still records your summary"),
        "{}",
        text(&tell)
    );

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);
    assert!(done.status.success(), "{}", text(&done));
    assert!(
        text(&done).contains("no orchestrator is registered in this workspace"),
        "{}",
        text(&done)
    );
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert!(
        !h.state.join("mail/w1/pending").exists(),
        "no recipient means no durable report"
    );
}

/// G5: a worker pane closed without `horch done`. `horch inbox` marks the
/// role, and `horch spawn --resume` ends the record ("pane closed without
/// horch done") and resumes it. While the pane is open, the resume refuses.
#[test]
fn g5_resume_after_a_closed_pane() {
    let h = fleet("g5-resume", "exec,stay");
    let (pane, role, id) = running_worker(&h);

    let out = h.run(&[
        "spawn",
        "--resume",
        &id,
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(!out.status.success(), "the pane is open: {}", text(&out));
    assert!(
        text(&out).contains(&format!("horch ledger legacy done {id}")),
        "{}",
        text(&out)
    );

    let closed = herdr(&h, &["pane", "close", &pane]);
    assert!(closed.status.success(), "{}", text(&closed));
    assert_eq!(record(&h, &id)["status"], "working", "no horch done ran");

    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    let line = stdout
        .lines()
        .find(|l| l.starts_with(&role))
        .unwrap_or_else(|| panic!("no {role} in {stdout}"));
    assert!(line.ends_with("(pane closed)"), "{stdout}");

    let pane2 = spawn(&h, &["--resume", &id, "keep going"]);
    let r = record(&h, &id);
    let events: Vec<(&str, &str)> = r["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["event"].as_str().unwrap(),
                e["text"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert!(
        events.contains(&("ended", "pane closed without horch done")),
        "{r}"
    );
    assert_eq!(events.last().unwrap().0, "resumed", "{r}");

    let role2 = r["role"].as_str().unwrap().to_string();
    done(&h, &pane2, &role2, &id);
}

/// MAIL-U1: the mail directory cannot be created, so enqueue fails.
/// `horch done` exits non-zero and says what to do; the record stays live
/// with a `report-failed` event, the role stays registered and the pane
/// stays open. The retry, once enqueue works, reports and records once.
#[test]
fn mail_u1_done_stays_open_when_enqueue_fails() {
    let h = fleet("mail-enqueue", "exec,stay");
    let (pane, role, id) = running_worker(&h);
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::create_dir_all(h.state.join("mail/w1")).unwrap();
    std::fs::write(h.state.join("mail/w1/pending"), "not a directory").unwrap();

    let failed = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);
    assert!(!failed.status.success(), "{}", text(&failed));
    assert!(
        text(&failed).contains("the report did not reach the orchestrator"),
        "{}",
        text(&failed)
    );
    assert!(
        text(&failed).contains("This pane stays open"),
        "{}",
        text(&failed)
    );
    let r = record(&h, &id);
    assert_eq!(r["status"], "working", "{r}");
    let events: Vec<&str> = r["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["event"].as_str())
        .collect();
    assert_eq!(events.last(), Some(&"report-failed"), "{r}");
    assert!(!events.contains(&"done"), "{r}");
    assert!(
        mailbox.join(format!("{role}.id")).exists(),
        "role stays registered"
    );
    assert!(
        herdr(&h, &["pane", "get", &pane]).status.success(),
        "the pane stays open"
    );

    std::fs::remove_file(h.state.join("mail/w1/pending")).unwrap();
    let mut retry = h.horch(&["done", "finished the work"]);
    retry
        .env("HERDR_PANE_ID", &pane)
        .env("HORCH_ROLE", &role)
        .env("HORCH_RECORD_ID", &id);
    let retried = retry.output().unwrap();
    assert!(retried.status.success(), "{}", text(&retried));
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    let dones = r["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["event"] == "done")
        .count();
    assert_eq!(dones, 1, "{r}");
}

/// MAIL-U1: once the report is durable, a failed push records and closes.
#[test]
fn mail_u1_done_push_failure_records_queued_and_closes() {
    let h = fleet("mail-done", "exec,stay,fail_typing");
    let (pane, role, id) = running_worker(&h);
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);

    assert!(done.status.success(), "{}", text(&done));
    assert!(
        String::from_utf8_lossy(&done.stderr).contains("horch done: the push did not land ("),
        "{}",
        text(&done)
    );
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    let events: Vec<&str> = r["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["event"].as_str())
        .collect();
    assert!(events.contains(&"report-queued"), "{r}");
    assert!(!events.contains(&"report-failed"), "{r}");
    let pending = h.state.join("mail/w1/pending");
    assert_eq!(std::fs::read_dir(pending).unwrap().count(), 1);
}

/// MAIL-U1: a failed push is not a failed worker message once its exact line
/// is durable. The courier note tells the caller why exit status is zero.
#[test]
fn mail_u1_tell_push_failure_leaves_pending_mail() {
    let h = fleet("mail-tell", "exec,stay,fail_typing");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();

    let tell = as_worker(
        &h,
        &["tell", "orchestrator", "NOTE: durable"],
        "w1:p2",
        "sonnet-1",
        "r1",
    );

    assert!(tell.status.success(), "{}", text(&tell));
    assert!(
        String::from_utf8_lossy(&tell.stderr).contains("the courier will redeliver it"),
        "{}",
        text(&tell)
    );
    let pending = h.state.join("mail/w1/pending");
    let files: Vec<_> = std::fs::read_dir(&pending)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let message: Value =
        serde_json::from_str(&std::fs::read_to_string(&files[0]).unwrap()).unwrap();
    assert_eq!(message["from"], "sonnet-1");
    assert_eq!(message["text"], "[sonnet-1] NOTE: durable");
}

#[test]
fn mail_u1_orchestrator_inbox_prints_then_acks_pending_mail() {
    let h = fleet("mail-inbox", "exec,stay");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::write(mailbox.join("sonnet-1.id"), "w1:p2").unwrap();
    for message in ["NOTE: first", "DONE: second"] {
        let tell = as_worker(
            &h,
            &["tell", "orchestrator", message],
            "w1:p2",
            "sonnet-1",
            "r1",
        );
        assert!(tell.status.success(), "{}", text(&tell));
    }

    let inbox = h.run(&["inbox"]);

    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    let first = stdout.find("[sonnet-1] NOTE: first").unwrap();
    let second = stdout.find("[sonnet-1] DONE: second").unwrap();
    let roles = stdout.find("orchestrator ").unwrap();
    assert!(roles < first && first < second, "{stdout}");
    assert_eq!(stdout.lines().last(), Some("2 messages acknowledged"));
    assert_eq!(
        std::fs::read_dir(h.state.join("mail/w1/pending"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(h.state.join("mail/w1/acked"))
            .unwrap()
            .count(),
        2
    );
}

/// Queue 3 worker messages for the orchestrator and return the harness.
fn inbox_fleet_with_three_messages(name: &str) -> Harness {
    let h = fleet(name, "exec,stay");
    let mailbox = h.tmp.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::write(mailbox.join("sonnet-1.id"), "w1:p2").unwrap();
    for message in ["NOTE: one", "NOTE: two", "DONE: three"] {
        let tell = as_worker(
            &h,
            &["tell", "orchestrator", message],
            "w1:p2",
            "sonnet-1",
            "r1",
        );
        assert!(tell.status.success(), "{}", text(&tell));
    }
    h
}

fn pending_count(h: &Harness) -> usize {
    std::fs::read_dir(h.state.join("mail/w1/pending"))
        .unwrap()
        .count()
}

#[test]
fn inbox_ends_with_the_acknowledged_count_after_the_roles() {
    let h = inbox_fleet_with_three_messages("inbox-count");

    let inbox = h.run(&["inbox"]);

    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    assert_eq!(stdout.lines().last(), Some("3 messages acknowledged"));
    assert!(
        stdout.find("orchestrator ").unwrap() < stdout.find("NOTE: one").unwrap(),
        "{stdout}"
    );
    assert_eq!(pending_count(&h), 0);
}

#[test]
fn inbox_roles_prints_no_mail_and_leaves_it_pending() {
    let h = inbox_fleet_with_three_messages("inbox-roles");

    let inbox = h.run(&["inbox", "--roles"]);

    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    assert!(stdout.contains("orchestrator "), "{stdout}");
    assert!(
        !stdout.contains("NOTE:") && !stdout.contains("DONE:"),
        "{stdout}"
    );
    assert!(!stdout.contains("acknowledged"), "{stdout}");
    assert_eq!(pending_count(&h), 3);
}

#[test]
fn inbox_peek_prints_mail_and_leaves_it_pending() {
    let h = inbox_fleet_with_three_messages("inbox-peek");

    let inbox = h.run(&["inbox", "--peek"]);

    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    for message in ["NOTE: one", "NOTE: two", "DONE: three"] {
        assert!(stdout.contains(message), "{stdout}");
    }
    assert!(!stdout.contains("messages acknowledged"), "{stdout}");
    assert_eq!(pending_count(&h), 3);
}

#[test]
fn mail_u1_worker_inbox_does_not_print_or_ack_pending_mail() {
    let h = fleet("mail-worker", "exec,stay");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    let tell = as_worker(
        &h,
        &["tell", "orchestrator", "NOTE: private"],
        "w1:p2",
        "sonnet-1",
        "r1",
    );
    assert!(tell.status.success(), "{}", text(&tell));

    let inbox = as_worker(&h, &["inbox"], "w1:p2", "sonnet-1", "r1");

    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        !String::from_utf8_lossy(&inbox.stdout).contains("NOTE: private"),
        "{}",
        text(&inbox)
    );
    assert_eq!(
        std::fs::read_dir(h.state.join("mail/w1/pending"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn mail_u1_orchestrator_command_reminds_without_acking() {
    let h = fleet("mail-remind", "exec,stay");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    for message in ["NOTE: first", "NOTE: second"] {
        let tell = as_worker(
            &h,
            &["tell", "orchestrator", message],
            "w1:p2",
            "sonnet-1",
            "r1",
        );
        assert!(tell.status.success(), "{}", text(&tell));
    }

    let sessions = h.run(&["sessions"]);

    assert!(sessions.status.success(), "{}", text(&sessions));
    assert!(
        String::from_utf8_lossy(&sessions.stderr)
            .contains("horch: 2 messages are not acknowledged; run horch inbox"),
        "{}",
        text(&sessions)
    );
    assert_eq!(
        std::fs::read_dir(h.state.join("mail/w1/pending"))
            .unwrap()
            .count(),
        2
    );
}

/// W2: the codex agent exits 0 at once, without `horch done` (as Codex
/// can at a usage limit or a crash). The record ends with an `exited` event,
/// and the worker types a `BLOCKED:` line into the orchestrator's pane.
#[test]
fn w2_agent_exit_before_done_tells_the_orchestrator() {
    let h = fleet("w2-early-exit", "exec");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    let pane = spawn(&h, &["codex-sol", "build the thing"]);
    let id = records(&h).pop().unwrap()["record_id"]
        .as_str()
        .unwrap()
        .to_string();

    let blocked = wait_for_or(
        "the BLOCKED line",
        || {
            h.calls_of("herdr").into_iter().find_map(|call| {
                let argv: Vec<String> = call["argv"]
                    .as_array()?
                    .iter()
                    .filter_map(|a| a.as_str().map(str::to_owned))
                    .collect();
                (argv.len() == 4 && argv[..3] == ["agent", "prompt", "w1:p1"])
                    .then(|| argv[3].clone())
                    .filter(|line| line.contains("BLOCKED:"))
            })
        },
        || pane_context(&h, &pane, &id),
    );
    assert!(
        blocked.ends_with(&format!(
            "BLOCKED: My agent exited with code 0 before horch done. The work can be \
             incomplete. Resume it with: horch spawn --resume {id}"
        )),
        "{blocked}"
    );
    let r = record(&h, &id);
    let last = r["history"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["event"], "exited", "{r}");
}

/// R1: a long `horch tell` spools its text under `messages/<workspace>/`, not
/// in a directory shared by every fleet, and the line typed into the
/// orchestrator's pane names that file.
#[test]
fn r1_long_tell_spools_under_its_own_workspace() {
    let h = fleet("r1-spool", "exec");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();

    let long = format!("QUESTION: {}", "Which branch lands first? ".repeat(40));
    let tell = as_worker(
        &h,
        &["tell", "orchestrator", &long],
        "w1:p2",
        "opus-1",
        "r1",
    );
    assert!(tell.status.success(), "{}", text(&tell));

    let messages = h.state.join("messages");
    let files_in = |dir: &std::path::Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .map(|d| {
                d.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default()
    };
    let spooled = files_in(&messages.join("w1"));
    assert_eq!(spooled.len(), 1, "{:?}", files_in(&messages));
    assert!(
        files_in(&messages).is_empty(),
        "a file sits directly under messages/: {:?}",
        files_in(&messages)
    );
    let body = std::fs::read_to_string(&spooled[0]).unwrap();
    assert!(
        body.starts_with("<!-- horch workspace=w1 from=opus-1 to=orchestrator -->\n"),
        "{body}"
    );

    let typed = h
        .calls_of("herdr")
        .into_iter()
        .filter_map(|call| {
            let argv: Vec<String> = call["argv"]
                .as_array()?
                .iter()
                .filter_map(|a| a.as_str().map(str::to_owned))
                .collect();
            (argv.len() == 4 && argv[..3] == ["agent", "prompt", "w1:p1"]).then(|| argv[3].clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert!(
        typed[0].contains(&spooled[0].display().to_string()),
        "the typed line does not name {}: {}",
        spooled[0].display(),
        typed[0]
    );
}

/// P09: a spawn writes the registry under the state folder and nothing under
/// the temp folder. With the temp folder emptied, `inbox` still lists the
/// role and `tell` still reaches it.
#[test]
fn p09_registry_lives_in_the_state_folder() {
    let h = fleet("p09-state", "exec,stay");
    let (_pane, role, _id) = running_worker(&h);

    assert!(h
        .state
        .join(format!("herdr-orchestration-w1/{role}.id"))
        .exists());
    assert!(h
        .state
        .join(format!("herdr-orchestration-w1/{role}.brief.json"))
        .exists());
    assert!(!h.tmp.join("herdr-orchestration-w1").exists());

    for entry in std::fs::read_dir(&h.tmp).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
    }

    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        String::from_utf8_lossy(&inbox.stdout)
            .lines()
            .any(|l| l.starts_with(&role)),
        "{}",
        text(&inbox)
    );
    let tell = h.run(&["tell", &role, "NOTE: still there"]);
    assert!(tell.status.success(), "{}", text(&tell));
}

/// P09: a registry that exists only in the old temp location is still found
/// by `inbox`, `tell` and `done`, and nothing is written back to it.
#[test]
fn p09_registry_in_the_old_temp_location_still_resolves() {
    let h = fleet("p09-legacy", "exec,stay");
    let (pane, role, id) = running_worker(&h);
    let state_mailbox = h.state.join("herdr-orchestration-w1");
    let legacy_mailbox = h.tmp.join("herdr-orchestration-w1");
    std::fs::rename(&state_mailbox, &legacy_mailbox).unwrap();

    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        String::from_utf8_lossy(&inbox.stdout)
            .lines()
            .any(|l| l.starts_with(&role)),
        "{}",
        text(&inbox)
    );
    let tell = h.run(&["tell", &role, "NOTE: old registry"]);
    assert!(tell.status.success(), "{}", text(&tell));

    done(&h, &pane, &role, &id);
    assert_eq!(record(&h, &id)["status"], "done");
    assert!(!legacy_mailbox.join(format!("{role}.id")).exists());
    assert!(!state_mailbox.join(format!("{role}.id")).exists());
}

/// A fleet with a custom `HORCH_STATE_DIR`: the orchestrator pane does not
/// inherit it, so its launch line has to carry it. The orchestrator registers
/// under that folder, not under the default state root, and `inbox` lists it.
#[cfg(unix)]
#[test]
fn t11_followup_orchestrator_registers_under_a_custom_state_dir() {
    let h = Harness::new("custom-state");
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["fleet", "--cwd", &project]);
    assert!(out.status.success(), "fleet: {}", text(&out));
    let launch = h
        .calls_of("herdr")
        .into_iter()
        .find(|c| {
            c["ran"]
                .as_str()
                .is_some_and(|l| l.contains("'pane-launch'"))
        })
        .expect("horch fleet ran a pane-launch command");
    let line = launch["ran"].as_str().unwrap();
    let pane = launch["argv"][2].as_str().unwrap();
    let workspace = pane.split(':').next().unwrap();

    // The pane is a fresh shell: it has no `HORCH_STATE_DIR` of its own.
    let mut shell = std::process::Command::new("/bin/sh");
    h.seal(&mut shell);
    shell
        .arg("-c")
        .arg(line)
        .env("HERDR_PANE_ID", pane)
        .env_remove("HORCH_STATE_DIR");
    let out = shell.output().unwrap();
    assert!(out.status.success(), "{line}\n{}", text(&out));

    let id = h
        .state
        .join(format!("herdr-orchestration-{workspace}/orchestrator.id"));
    assert!(id.is_file(), "no {}\n{}", id.display(), text(&out));
    let default_root = h.home.join(".local/state/horch");
    assert!(
        !default_root.exists(),
        "the pane wrote under the default state root: {}",
        default_root.display()
    );

    let mut inbox = h.horch(&["inbox"]);
    inbox.env("HORCH_WORKSPACE_ID", workspace);
    let inbox = inbox.output().unwrap();
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        String::from_utf8_lossy(&inbox.stdout)
            .lines()
            .any(|l| l.starts_with("orchestrator")),
        "{}",
        text(&inbox)
    );
}

/// A mailbox removal that fails does not stop `horch done`: it warns with the
/// role and the path, and still closes the pane.
#[cfg(unix)]
#[test]
fn t11_followup_done_warns_when_the_registration_stays() {
    let h = fleet("done-warns", "exec,stay");
    let (pane, role, id) = running_worker(&h);
    let id_file = h.state.join(format!("herdr-orchestration-w1/{role}.id"));
    std::fs::remove_file(&id_file).unwrap();
    std::fs::create_dir_all(id_file.join("not-a-file")).unwrap();

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);

    assert!(done.status.success(), "{}", text(&done));
    let stderr = String::from_utf8_lossy(&done.stderr);
    let warning = stderr
        .lines()
        .find(|l| l.starts_with("horch done:") && l.contains(&id_file.display().to_string()))
        .unwrap_or_else(|| panic!("no warning in {}", text(&done)));
    assert!(warning.contains(&role), "{warning}");
    assert_eq!(record(&h, &id)["status"], "done");
    let panes = herdr(&h, &["pane", "list"]);
    assert!(
        !String::from_utf8_lossy(&panes.stdout).contains(&pane),
        "{}",
        text(&panes)
    );
}
