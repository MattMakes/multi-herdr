//! #125: herdr resumes agent panes by itself after a server restart. A
//! resumed worker pane runs its agent without `horch worker`, so it has no
//! `HORCH_ROLE`, `HORCH_RECORD_ID` or `HORCH_WORKSPACE_ID`. horch finds its
//! registered identity, and `horch fleet` and `horch doctor` warn that the
//! restore is on.
#![cfg(unix)]

use std::process::Output;

use horch_core::messaging::mail::Mail;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Workspace `w1` with its root pane `w1:p1`, as `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", "20");
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    herdr(&h, &["workspace", "create", "--label", name]);
    h
}

fn herdr(h: &Harness, args: &[&str]) {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
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

/// A spawned codex worker that runs, as `(pane, role, record id)`.
fn running_worker(h: &Harness) -> (String, String, String) {
    let o = h.run(&[
        "spawn",
        "codex-sol",
        "build the thing",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let pane = String::from_utf8_lossy(&o.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();
    let r = records(h).pop().expect("the spawn wrote a record");
    let id = r["record_id"].as_str().unwrap().to_string();
    let role = r["role"].as_str().unwrap().to_string();
    wait_for_or(
        "the running worker",
        || {
            let r = record(h, &id);
            (r["state"]["state"] == "running" && r["session_id"].is_string()).then_some(())
        },
        || format!("{}", record(h, &id)),
    );
    (pane, role, id)
}

/// `horch` as an agent that herdr resumed in `pane` runs it: herdr's pane
/// id, and none of the variables `horch worker` gives its agent.
fn restored(h: &Harness, pane: &str, args: &[&str]) -> Output {
    h.horch(args)
        .env("HERDR_PANE_ID", pane)
        .env_remove("HORCH_ROLE")
        .env_remove("HORCH_RECORD_ID")
        .env_remove("HORCH_WORKER_ID")
        .env_remove("HORCH_WORKSPACE_ID")
        .output()
        .unwrap()
}

fn pending(h: &Harness) -> Vec<horch_core::messaging::mail::Message> {
    Mail::under(&h.state, "w1").pending().unwrap_or_default()
}

#[test]
fn a_restored_worker_pane_tells_as_its_role_peeks_its_inbox_and_finishes() {
    let h = fleet("r125-restored", "exec,stay");
    let (pane, role, id) = running_worker(&h);
    // herdr resumed the agent: it shows the agent, and the pane is still
    // registered for the role.
    herdr(
        &h,
        &[
            "pane",
            "test-state",
            &pane,
            "--text",
            "Ready",
            "--status",
            "idle",
            "--agent",
            "codex",
        ],
    );
    let tell = restored(&h, &pane, &["tell", "orchestrator", "QUESTION: which API?"]);
    assert!(tell.status.success(), "{}", text(&tell));
    let err = String::from_utf8_lossy(&tell.stderr);
    assert!(
        err.contains(&format!("runs {role} outside its horch worker supervisor"))
            && err.contains(&format!("horch spawn --resume {id}")),
        "{err}"
    );
    let mail = pending(&h);
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert_eq!(mail[0].from, role);
    assert!(
        mail[0]
            .text
            .starts_with(&format!("[{role}] QUESTION: which API?")),
        "{}",
        mail[0].text
    );

    let inbox = restored(&h, &pane, &["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    // The open question is listed; the mail itself is not printed.
    assert!(
        !String::from_utf8_lossy(&inbox.stdout).contains(&format!("[{role}] QUESTION")),
        "{}",
        text(&inbox)
    );
    assert_eq!(
        pending(&h).len(),
        1,
        "the worker's inbox acknowledges nothing"
    );

    let done = restored(&h, &pane, &["done", "finished the work"]);
    assert!(done.status.success(), "{}", text(&done));
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert!(r.to_string().contains("finished the work"), "{r}");
}

#[test]
fn an_operator_shell_on_a_registered_pane_id_stays_the_operator() {
    let h = fleet("r125-operator", "exec,stay");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::write(mailbox.join("sonnet-1.id"), "w1:p1").unwrap();
    // The orchestrator's pane runs no agent herdr detects: an untagged line
    // is the operator's, and its inbox acknowledges.
    let tell = restored(&h, "w1:p1", &["tell", "orchestrator", "NOTE: from me"]);
    assert!(tell.status.success(), "{}", text(&tell));
    assert!(
        !String::from_utf8_lossy(&tell.stderr).contains("outside its horch worker supervisor"),
        "{}",
        text(&tell)
    );
    assert!(pending(&h).is_empty(), "an operator line is not mail");
}

fn doctor_text(h: &Harness) -> String {
    let o = h.run(&["doctor"]);
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn doctor_and_fleet_warn_while_herdr_resumes_agents_on_restore() {
    let mut h = Harness::new("r125-warn");
    h.set("HORCH_FAKE_SCENARIO", "exec,shell");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_FAULT", "delivery-probe-ms:4000");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    let config = h.home.join(".config/herdr/config.toml");
    let pinned = format!(
        "warning: herdr resumes agent panes by itself after a herdr server restart: \
         [session] resume_agents_on_restore is not false in {}. A resumed fleet agent \
         runs without its horch worker supervisor, so it loses its role, its reports and \
         its relay. It also starts from the herdr server's environment, which horch does \
         not scrub, so it can bill {} instead of the claude.ai login. Update herdr with \
         `herdr update --handoff`, and do not restart the herdr server while a fleet runs. \
         To turn the restore off, set resume_agents_on_restore = false under [session] in \
         that file.",
        config.display(),
        horch_core::harness::launch::FORBIDDEN_ENV[0]
    );
    // No config file: herdr's default resumes.
    assert!(doctor_text(&h).contains(&pinned), "{}", doctor_text(&h));
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "[session]\nresume_agents_on_restore = true\n").unwrap();
    assert!(doctor_text(&h).contains(&pinned));
    let fleet = h.run(&["fleet", "--no-update"]);
    assert!(
        String::from_utf8_lossy(&fleet.stderr).contains(&pinned),
        "{}",
        text(&fleet)
    );
    std::fs::write(&config, "[session]\nresume_agents_on_restore = false\n").unwrap();
    assert!(
        !doctor_text(&h).contains("resume_agents_on_restore"),
        "{}",
        doctor_text(&h)
    );
    // HERDR_CONFIG_PATH names another file, as for herdr itself.
    let other = h.root.join("herdr-other.toml");
    std::fs::write(&other, "").unwrap();
    h.set("HERDR_CONFIG_PATH", other.to_string_lossy());
    assert!(doctor_text(&h).contains(&format!(
        "resume_agents_on_restore is not false in {}",
        other.display()
    )));
}
