use std::process::Output;

use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::execution::{ExecutionStatus, ReportTarget, SessionMode};
use horch_core::messaging::brief::{Brief, SCHEMA};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::Roster;
use horch_core::runtime::BinOverrides;
use horch_core::skills::{harness_support, ResolvedSkillRef, SkillResolution};
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

fn worker_fleet(name: &str, scenario: &str) -> Harness {
    worker_fleet_with(name, scenario, |_| {})
}

fn worker_fleet_with(name: &str, scenario: &str, prepare: impl FnOnce(&mut Harness)) -> Harness {
    let mut h = fleet(name, scenario);
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WATCHDOG", "1");
    prepare(&mut h);
    h.serve_fleetd();
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
    h.stored_ledger(&slug)
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

fn pane_context(h: &Harness, pane: &str, id: &str) -> String {
    let mut out = h.log.clone().into_os_string();
    out.push(format!(".pane-{}.out", pane.replace(':', "_")));
    format!(
        "record: {}\npane output:\n{}",
        record(h, id),
        std::fs::read_to_string(&out).unwrap_or_else(|e| format!("({e})"))
    )
}

fn done(h: &Harness, pane: &str, role: &str, id: &str) {
    let mut cmd = h.horch(&["done", "finished the work"]);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

fn as_worker(h: &Harness, args: &[&str], pane: &str, role: &str, id: &str) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HERDR_PANE_ID", pane)
        .env("HORCH_ROLE", role)
        .env("HORCH_RECORD_ID", id);
    cmd.output().unwrap()
}

fn recorded_skills(h: &Harness, teammate: &str) -> Vec<ResolvedSkillRef> {
    let roster = Roster::builtin().unwrap();
    let launching = roster.for_launch(roster.get(teammate).unwrap().clone());
    SkillResolution::read(
        roster.skill_catalog().unwrap(),
        Some(&launching),
        Some(&h.home),
        harness_support,
    )
    .unwrap()
    .plan(&launching)
    .unwrap()
    .activated
}

fn launch_worker(h: &Harness) -> (String, String, String) {
    let split = herdr(
        h,
        &[
            "pane",
            "split",
            "w1:p1",
            "--direction",
            "right",
            "--no-focus",
        ],
    );
    assert!(split.status.success(), "{}", text(&split));
    let reply: Value = serde_json::from_slice(&split.stdout).unwrap();
    let pane = reply["result"]["pane"]["pane_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no pane in {reply}"))
        .to_string();
    let (role, id) = ("codex-sol-1".to_string(), "r-worker-1".to_string());
    let project = h.project.to_string_lossy().into_owned();
    SessionLedger::for_project(&h.state, &project)
        .insert(Record {
            record_id: id.clone(),
            agent: "codex".into(),
            tier: "codex-sol".into(),
            model: "gpt-5.6-sol".into(),
            role: role.clone(),
            task: "work".into(),
            pane_id: Some(pane.clone()),
            workspace_id: Some("w1".into()),
            skills: recorded_skills(h, "codex-sol"),
            state: Some(ExecutionStatus::Running),
            ..Record::default()
        })
        .unwrap();
    Mailbox::under(&h.state, "w1")
        .write_brief(&Brief {
            schema: SCHEMA,
            role: role.clone(),
            teammate: "codex-sol".into(),
            agent: "codex".into(),
            model: "gpt-5.6-sol".into(),
            record_id: id.clone(),
            session: SessionMode::Fresh(None),
            task: "work".into(),
            project_dir: project,
            state_dir: None,
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: None,
            workdir: None,
            bin_overrides: BinOverrides::default(),
            report_to: ReportTarget::Orchestrator,
            worker_id: None,
            ledger_db: None,
            fleet_root: None,
            controller: None,
            task_yaml: None,
            backend: Default::default(),
        })
        .unwrap();
    let worker = format!(
        "'{}' worker {role}",
        horch_e2e::harness::Harness::horch_bin().display()
    );
    let run = herdr(h, &["pane", "run", &pane, &worker]);
    assert!(run.status.success(), "{}", text(&run));
    (pane, role, id)
}

fn running_worker(h: &Harness) -> (String, String, String) {
    let (pane, role, id) = launch_worker(h);
    wait_for_or(
        "the running worker",
        || {
            h.calls_of("codex")
                .iter()
                .any(|c| c["session_id"].is_string())
                .then_some(())
        },
        || pane_context(h, &pane, &id),
    );
    (pane, role, id)
}

#[test]
fn g5_done_without_an_orchestrator() {
    let mut h = worker_fleet("g5-done", "exec,stay");
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    let (pane, role, id) = running_worker(&h);

    let tell = as_worker(&h, &["tell", "orchestrator", "DONE: x"], &pane, &role, &id);
    assert!(tell.status.success(), "{}", text(&tell));
    assert!(
        text(&tell).contains("queued, not yet acknowledged"),
        "{}",
        text(&tell)
    );

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);
    assert!(done.status.success(), "{}", text(&done));
    assert!(
        text(&done).contains("queued, not yet acknowledged"),
        "{}",
        text(&done)
    );
    let r = record(&h, &id);
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(
        std::fs::read_dir(h.state.join("mail/w1/pending"))
            .unwrap()
            .count(),
        2,
        "tell and done both remain pending without registration"
    );
}

#[test]
fn done_outside_a_git_work_tree_reports_no_scan_error() {
    let mut h = worker_fleet("done-no-repo", "exec,stay").with_git();
    if h.git_bin().is_none() {
        return;
    }
    std::fs::remove_dir_all(h.project.join(".git")).unwrap();
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    let (pane, role, id) = running_worker(&h);

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);
    assert!(done.status.success(), "{}", text(&done));
    assert!(
        !text(&done).contains("could not inspect unregistered worktrees"),
        "{}",
        text(&done)
    );
    assert_eq!(record(&h, &id)["status"], "done");
}

#[test]
fn g5_inbox_marks_a_role_whose_pane_closed() {
    let h = worker_fleet("g5-closed", "exec,stay");
    let (pane, role, id) = running_worker(&h);

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
}

#[test]
fn mail_u1_done_stays_open_when_enqueue_fails() {
    let h = worker_fleet("mail-enqueue", "exec,stay");
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

#[test]
fn mail_u1_done_push_failure_records_queued_and_closes() {
    let h = worker_fleet("mail-done", "exec,stay,fail_typing");
    let (pane, role, id) = running_worker(&h);
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();

    let done = as_worker(&h, &["done", "finished the work"], &pane, &role, &id);

    assert!(done.status.success(), "{}", text(&done));
    assert!(
        String::from_utf8_lossy(&done.stderr)
            .contains("horch done: queued, not yet acknowledged ("),
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
        String::from_utf8_lossy(&tell.stderr).contains("queued, not yet acknowledged"),
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
    let line = |text: &str| {
        stdout
            .lines()
            .find(|l| l.starts_with("[sonnet-1 #") && l.ends_with(&format!("] {text}")))
            .and_then(|l| stdout.find(l))
            .unwrap_or_else(|| panic!("no {text} line: {stdout}"))
    };
    let first = line("NOTE: first");
    let second = line("DONE: second");
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

#[cfg(unix)]
const PIDFILE_TOKEN: &str = "@PIDFILE@";

#[cfg(unix)]
fn inbox_with_broken_pane_list(name: &str, on_pane_list: &str) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mut h = inbox_fleet_with_three_messages(name);
    let script = h.tmp.join("broken-herdr");
    let pidfile = h.tmp.join("stalled.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nif [ \"$1 $2\" = \"pane list\" ]; then echo $$ > {pid}; {body}; fi\nexec {fake} \"$@\"\n",
            pid = pidfile.display(),
            body = on_pane_list.replace(PIDFILE_TOKEN, &pidfile.to_string_lossy()),
            fake = h.bin.join("herdr").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let warmed = horch_e2e::harness::warm(std::process::Command::new(&script).arg("warm")).unwrap();
    assert!(
        warmed.status.success(),
        "warming {} failed",
        script.display()
    );
    h.set("HORCH_HERDR_BIN", script.to_string_lossy());
    h.set("HORCH_TEST_PANE_LIST_MS", "1500");

    let started = std::time::Instant::now();
    let inbox = h.run(&["inbox"]);

    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(inbox.status.success(), "{}", text(&inbox));
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    let stderr = String::from_utf8_lossy(&inbox.stderr);
    assert!(stderr.contains("roles unavailable"), "{stderr}");
    assert!(stdout.contains("orchestrator "), "{stdout}");
    for message in ["NOTE: one", "NOTE: two", "DONE: three"] {
        assert!(stdout.contains(message), "{stdout}");
    }
    assert_eq!(stdout.lines().last(), Some("3 messages acknowledged"));
    assert_eq!(pending_count(&h), 0);
    std::fs::read_to_string(&pidfile).ok()
}

#[cfg(unix)]
fn assert_process_gone(pid: &str) {
    let gone = (0..40).any(|_| {
        let alive = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .unwrap()
            .status
            .success();
        if alive {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !alive
    });
    assert!(gone, "process {pid} is still running");
}

#[cfg(unix)]
#[test]
fn inbox_prints_mail_and_kills_a_herdr_that_stalls_on_pane_list() {
    if let Some(pid) = inbox_with_broken_pane_list("inbox-stall", "exec sleep 60") {
        assert_process_gone(&pid);
    }
}

#[cfg(unix)]
#[test]
fn inbox_prints_mail_and_kills_a_descendant_that_holds_the_pane_list_pipe() {
    if let Some(pid) = inbox_with_broken_pane_list(
        "inbox-open-pipe",
        &format!("sleep 30 &\necho $! > {PIDFILE_TOKEN}\nexit 0"),
    ) {
        assert_process_gone(&pid);
    }
}

#[cfg(unix)]
#[test]
fn inbox_prints_mail_when_pane_list_prints_more_than_1_mib() {
    inbox_with_broken_pane_list("inbox-flood", "exec head -c 3000000 /dev/zero");
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
    assert_eq!(
        stdout.lines().last(),
        Some("3 messages pending, not acknowledged")
    );
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
fn a_worker_inbox_prints_and_acks_its_own_role_mail_only() {
    let mut h = fleet("mail-worker-role", "exec,stay");
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::write(mailbox.join("sonnet-1.id"), "w1:p2").unwrap();
    let tell = as_worker(
        &h,
        &["tell", "orchestrator", "NOTE: private"],
        "w1:p2",
        "sonnet-1",
        "r1",
    );
    assert!(tell.status.success(), "{}", text(&tell));
    horch_core::messaging::mail::Mail::to_role(&h.state, "w1", "sonnet-1")
        .enqueue("horch", "NOTE: the reviews are in")
        .unwrap();
    let pending = |dir: &str| std::fs::read_dir(h.state.join(dir)).unwrap().count();

    let peek = as_worker(&h, &["inbox", "--peek"], "w1:p2", "sonnet-1", "r1");
    assert!(peek.status.success(), "{}", text(&peek));
    assert!(
        text(&peek).contains("NOTE: the reviews are in"),
        "{}",
        text(&peek)
    );
    assert_eq!(pending("mail/w1/roles/sonnet-1/pending"), 1);

    let inbox = as_worker(&h, &["inbox"], "w1:p2", "sonnet-1", "r1");
    let stdout = String::from_utf8_lossy(&inbox.stdout);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        stdout.contains("NOTE: the reviews are in"),
        "{}",
        text(&inbox)
    );
    assert!(!stdout.contains("NOTE: private"), "{}", text(&inbox));
    assert_eq!(pending("mail/w1/roles/sonnet-1/pending"), 0);
    assert_eq!(
        pending("mail/w1/pending"),
        1,
        "the orchestrator's mail stays"
    );
}

#[test]
fn a_courier_drops_the_mail_of_a_role_no_pane_holds_and_exits() {
    let h = fleet("mail-gone-role", "exec,stay");
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("sonnet-2.id"), "w1:p3").unwrap();
    horch_core::messaging::mail::Mail::to_role(&h.state, "w1", "sonnet-1")
        .enqueue("horch", "NOTE: the reviews are in")
        .unwrap();

    let courier = h
        .horch(&["mail", "courier", "--workspace", "w1"])
        .env("HORCH_TEST_MAIL_POLL_MS", "10")
        .output()
        .unwrap();

    assert!(courier.status.success(), "{}", text(&courier));
    let pending = h.state.join("mail/w1/roles/sonnet-1/pending");
    assert_eq!(std::fs::read_dir(pending).unwrap().count(), 0);
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

#[test]
fn inbox_roles_still_reminds_about_pending_mail() {
    let h = inbox_fleet_with_three_messages("inbox-roles-remind");

    let inbox = h.run(&["inbox", "--roles"]);

    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(
        String::from_utf8_lossy(&inbox.stderr)
            .contains("horch: 3 messages are not acknowledged; run horch inbox"),
        "{}",
        text(&inbox)
    );
    assert_eq!(pending_count(&h), 3);
}

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
    assert_eq!(
        typed,
        [horch_core::messaging::delivery::MAIL_LINE],
        "{typed:?}"
    );
    let mailed = horch_core::messaging::mail::Mail::under(&h.state, "w1")
        .pending()
        .unwrap();
    assert!(
        mailed[0].text.contains(&spooled[0].display().to_string()),
        "the mail does not name {}: {}",
        spooled[0].display(),
        mailed[0].text
    );
}

#[test]
fn p09_registry_lives_in_the_state_folder() {
    let h = worker_fleet("p09-state", "exec,stay");
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

#[cfg(unix)]
#[test]
fn p09_registry_in_the_old_temp_location_still_resolves() {
    use std::os::unix::fs::PermissionsExt;
    let h = worker_fleet("p09-legacy", "exec,stay");
    let (pane, role, id) = running_worker(&h);
    let state_mailbox = h.state.join("herdr-orchestration-w1");
    let legacy_mailbox = h.tmp.join("herdr-orchestration-w1");
    std::fs::rename(&state_mailbox, &legacy_mailbox).unwrap();
    std::fs::set_permissions(&legacy_mailbox, std::fs::Permissions::from_mode(0o700)).unwrap();

    let inbox = h.run(&["inbox"]);
    assert!(
        state_mailbox.join(format!("{role}.id")).is_file(),
        "the first command copies the registry"
    );
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

#[cfg(unix)]
#[test]
fn an_old_registry_others_can_write_is_not_copied_and_inbox_says_why() {
    use std::os::unix::fs::PermissionsExt;
    let h = worker_fleet("p09-legacy-open", "exec,stay");
    let (_pane, role, _id) = running_worker(&h);
    let state_mailbox = h.state.join("herdr-orchestration-w1");
    let legacy_mailbox = h.tmp.join("herdr-orchestration-w1");
    std::fs::rename(&state_mailbox, &legacy_mailbox).unwrap();
    std::fs::set_permissions(&legacy_mailbox, std::fs::Permissions::from_mode(0o777)).unwrap();

    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    let stderr = String::from_utf8_lossy(&inbox.stderr);
    assert!(
        stderr.contains(&legacy_mailbox.display().to_string())
            && stderr.contains("writable by other users"),
        "{}",
        text(&inbox)
    );
    assert!(
        !String::from_utf8_lossy(&inbox.stdout)
            .lines()
            .any(|l| l.starts_with(&role)),
        "{}",
        text(&inbox)
    );
    assert!(!state_mailbox.exists(), "nothing was copied");
    let tell = h.run(&["tell", &role, "NOTE: old registry"]);
    assert!(!tell.status.success(), "{}", text(&tell));
}

#[cfg(unix)]
#[test]
fn t11_followup_done_warns_when_the_registration_stays() {
    let h = worker_fleet("done-warns", "exec,stay");
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
