//! #125: herdr resumes agent panes by itself after a server restart. A
//! resumed worker pane runs its agent without `horch worker`, so it has no
//! `HORCH_ROLE`, `HORCH_RECORD_ID` or `HORCH_WORKSPACE_ID`. horch finds its
//! registered identity, and `horch fleet` and `horch doctor` warn that the
//! restore is on.
#![cfg(unix)]

use std::process::Output;

use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::messaging::brief::Brief;
use horch_core::messaging::mail::Mail;
use horch_core::messaging::mailbox::Mailbox;
use horch_e2e::harness::{fixtures, Harness};
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
    // The session records and the task ledger share 1 file below the state
    // folder, as in production: the test writes the records of the worker
    // through the test fleetd of that file.
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h.set("HORCH_NO_WATCHDOG", "1");
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

/// The session record of the worker, as the session ledger holds it.
fn record(h: &Harness, id: &str) -> Value {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .get(id)
        .map(|r| serde_json::to_value(r).unwrap())
        .unwrap()
}

/// A registered codex worker, as `(pane, role, record id)`: the pane
/// beside the root pane, the registry entry of its role, its brief and
/// its session record. `horch spawn` no longer starts a worker outside a
/// run, so the test writes what a spawn left, and the test fleetd of the
/// harness writes the records that the commands change.
fn running_worker(h: &Harness) -> (String, String, String) {
    let (role, id) = ("codex-sol-1", "r-restored");
    let mut split = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut split);
    let o = split
        .args(["pane", "split", "w1:p1", "--direction", "right"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let reply: Value = serde_json::from_slice(&o.stdout).unwrap();
    let pane = reply["result"]["pane"]["pane_id"]
        .as_str()
        .expect("the reply names the new pane")
        .to_string();

    let mailbox = Mailbox::under(&h.state, "w1");
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    mailbox.register_role(role, &pane).unwrap();
    let brief: Brief = serde_json::from_value(serde_json::json!({
        "schema": 2,
        "role": role,
        "teammate": "codex-sol",
        "agent": "codex",
        "model": "gpt-5.6-sol",
        "record_id": id,
        "session_id": "",
        "resume": false,
        "project_dir": h.project,
    }))
    .unwrap();
    mailbox.write_brief(&brief).unwrap();
    h.serve_fleetd();
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: id.into(),
            agent: "codex".into(),
            tier: "codex-sol".into(),
            model: "gpt-5.6-sol".into(),
            role: role.into(),
            task: "work".into(),
            ..Record::default()
        })
        .unwrap();
    (pane, role.into(), id.into())
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

/// Stop the test fleetd of `h`, then the jobs that it started: a job
/// outlives its fleetd, fleetd runs a docs and a conformance job for each
/// fake CLI at its first pass, and the teardown fails a test whose job
/// still runs. A job gets SIGTERM, and SIGKILL after 10 s. Each signal
/// names the PID and the start time.
fn stop_fleetd_and_jobs(h: &Harness) {
    use horch_core::procid::{signal_same, start_time};
    h.stop_fleetd();
    let marks: Vec<String> = [Some(h.root.clone()), std::fs::canonicalize(&h.root).ok()]
        .into_iter()
        .flatten()
        .map(|root| format!("{}/", root.display()))
        .collect();
    let jobs = || -> Vec<(u32, u64)> {
        horch_e2e::process::commands_with_environment()
            .into_iter()
            .filter(|(_, l)| {
                l.contains(" job run ") && marks.iter().any(|m| l.contains(m.as_str()))
            })
            .filter_map(|(pid, _)| Some((pid, start_time(pid)?)))
            .collect()
    };
    let first = std::time::Instant::now();
    horch_e2e::harness::wait_for("the jobs of fleetd to end", || {
        let running = jobs();
        let signal = if first.elapsed() < std::time::Duration::from_secs(10) {
            15
        } else {
            9
        };
        for (pid, started) in &running {
            signal_same(*pid, *started, signal);
        }
        running.is_empty().then_some(())
    });
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
    stop_fleetd_and_jobs(&h);
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
    // FR-7: the operator line waits in mail too, untagged and from the
    // operator, not from the role of the pane.
    let mail = pending(&h);
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert_eq!(mail[0].from, "operator");
    assert_eq!(mail[0].text, "NOTE: from me");
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
