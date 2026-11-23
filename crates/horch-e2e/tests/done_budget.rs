#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{wait_for, Harness};
use serde_json::{json, Value};

const WORKSPACE: &str = "w1";
const ORCHESTRATOR: &str = "w1:p1";
const WORKER: &str = "w2:p1";
const ROLE: &str = "opus-1";
const RECORD: &str = "r-done";
const CLOSING: &str = "closing: reporting to the orchestrator";
const BUDGET: Duration = Duration::from_secs(2);
const BOUNDED: Duration = Duration::from_secs(10);

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

fn stick_orchestrator(h: &Harness) {
    let mut path = h.log.clone().into_os_string();
    path.push(".state.json");
    let path = PathBuf::from(path);
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for ws in state["workspaces"].as_array_mut().unwrap() {
        for pane in ws["panes"].as_array_mut().unwrap() {
            if pane["pane_id"] == ORCHESTRATOR {
                pane["agent"] = json!("claude");
                pane["agent_status"] = json!("unknown");
            }
        }
    }
    std::fs::write(&path, state.to_string()).unwrap();
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_WORKSPACE_ID", WORKSPACE);
    for label in ["orchestrator", "worker"] {
        let o = herdr(&h, &["workspace", "create", "--label", label]);
        assert!(o.status.success(), "{}", text(&o));
    }
    let mailbox = h.tmp.join(format!("herdr-orchestration-{WORKSPACE}"));
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), ORCHESTRATOR).unwrap();
    std::fs::write(mailbox.join(format!("{ROLE}.id")), WORKER).unwrap();
    stick_orchestrator(&h);
    h.serve_fleetd();
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: RECORD.into(),
            agent: "claude".into(),
            tier: "opus".into(),
            model: "opus".into(),
            role: ROLE.into(),
            task: "work".into(),
            ..Record::default()
        })
        .unwrap();
    h
}

fn as_worker(h: &Harness, args: &[&str]) -> Command {
    let mut cmd = h.horch(args);
    cmd.env("HERDR_PANE_ID", WORKER)
        .env("HORCH_ROLE", ROLE)
        .env("HORCH_RECORD_ID", RECORD);
    cmd
}

fn short_budget(h: &mut Harness) {
    h.set(
        "HORCH_FAULT",
        format!("done-push-budget-ms:{}", BUDGET.as_millis()),
    );
}

fn assert_bounded(what: &str, took: Duration, o: &Output) {
    assert!(
        took >= BUDGET,
        "{what} took {took:?}, less than the {BUDGET:?} budget: {}",
        text(o)
    );
    assert!(took < BOUNDED, "{what} took {took:?}: {}", text(o));
}

fn record(h: &Harness) -> Value {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .get(RECORD)
        .map(|r| serde_json::to_value(r).unwrap())
        .unwrap()
}

fn done_entries(h: &Harness) -> usize {
    record(h)["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["event"] == "done")
        .count()
}

fn pending(h: &Harness) -> Vec<String> {
    let dir = h.state.join(format!("mail/{WORKSPACE}/pending"));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| match std::fs::read_to_string(e.unwrap().path()) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("reading pending mail: {error}"),
        })
        .collect()
}

fn worker_pane_closed(h: &Harness) -> bool {
    !herdr(h, &["pane", "get", WORKER]).status.success()
}

#[test]
fn done_finishes_in_time_against_a_stuck_orchestrator() {
    let mut h = world("done-stuck");
    short_budget(&mut h);
    let started = Instant::now();
    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    let took = started.elapsed();
    assert!(o.status.success(), "{}", text(&o));
    assert_bounded("done", took, &o);
    assert_eq!(
        String::from_utf8_lossy(&o.stderr).lines().next(),
        Some(CLOSING),
        "{}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("queued, not yet acknowledged")
            && String::from_utf8_lossy(&o.stderr)
                .contains("redelivered until horch inbox reads it"),
        "{}",
        text(&o)
    );
    let mail = pending(&h);
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert!(
        mail[0].contains("[opus-1] DONE: finished the work"),
        "{mail:?}"
    );
    assert_eq!(record(&h)["status"], "done", "{}", record(&h));
    assert!(worker_pane_closed(&h), "the worker pane closes");
}

struct Wrapper {
    sh: std::process::Child,
    runs: usize,
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        drop(self.sh.stdin.take());
        let _ = self.sh.wait();
    }
}

fn quoted(text: &std::ffi::OsStr) -> String {
    format!("'{}'", text.to_string_lossy().replace('\'', "'\\''"))
}

impl Wrapper {
    fn start() -> Wrapper {
        let sh = Command::new("/bin/sh")
            .args(["-c", "while IFS= read -r line; do eval \"$line\"; done"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        Wrapper { sh, runs: 0 }
    }

    fn run(&mut self, h: &Harness, cmd: &Command) -> Output {
        use std::io::Write as _;
        use std::os::unix::process::ExitStatusExt as _;
        self.runs += 1;
        let base = h.root.join(format!("wrapper-{}", self.runs));
        let [out, err, status] = ["out", "err", "status"].map(|e| base.with_extension(e));
        let env: Vec<String> = cmd
            .get_envs()
            .filter_map(|(k, v)| v.map(|v| format!("{}={}", k.to_string_lossy(), quoted(v))))
            .collect();
        let args: Vec<String> = cmd.get_args().map(quoted).collect();
        let line = format!(
            "{{ env -i {} {} {}; }} > {} 2> {}; echo $? > {}.tmp; mv {}.tmp {}",
            env.join(" "),
            quoted(cmd.get_program()),
            args.join(" "),
            quoted(out.as_os_str()),
            quoted(err.as_os_str()),
            quoted(status.as_os_str()),
            quoted(status.as_os_str()),
            quoted(status.as_os_str()),
        );
        let stdin = self.sh.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let code = wait_for("the command of the wrapper to end", || {
            std::fs::read_to_string(&status).ok()
        });
        Output {
            status: std::process::ExitStatus::from_raw(code.trim().parse::<i32>().unwrap() << 8),
            stdout: std::fs::read(&out).unwrap_or_default(),
            stderr: std::fs::read(&err).unwrap_or_default(),
        }
    }
}

fn bound_task(h: &Harness) -> Wrapper {
    let run = horch_e2e::ledger_seed::seed_run(h, "alpha", None, ORCHESTRATOR);
    let ticket = h.project.join("01-a.md");
    std::fs::write(
        &ticket,
        "# 01: Feature\n\n**Persona:** primary `opus`\n**Check:** `true`\n",
    )
    .unwrap();
    horch_e2e::ledger_seed::seed_task(h, &run, &ticket, "opus");
    let wrapper = Wrapper::start();
    let bound = horch_e2e::bind::bind(h, "opus", Some(wrapper.sh.id()));
    std::fs::create_dir_all(
        h.fleet_root()
            .join("tasks")
            .join(&bound.task_id)
            .join("attempts")
            .join(format!("A{}", bound.attempt)),
    )
    .unwrap();
    wrapper
}

fn results(h: &Harness) -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.file_name().is_some_and(|n| n == "result.json") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&h.fleet_root().join("tasks"), &mut out);
    out
}

#[test]
fn ledger_done_finishes_in_time_against_a_stuck_orchestrator() {
    let mut h = world("ldone-stuck");
    let mut wrapper = bound_task(&h);
    short_budget(&mut h);

    let started = Instant::now();
    let o = wrapper.run(&h, &as_worker(&h, &["ledger", "done", "finished the task"]));
    let took = started.elapsed();
    assert!(o.status.success(), "{}", text(&o));
    assert!(took < BOUNDED, "ledger done took {took:?}: {}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("fleetd runs the finish steps"),
        "{}",
        text(&o)
    );
    let found = results(&h);
    assert_eq!(found.len(), 1, "{found:?}");
    let result: Value = serde_json::from_str(&std::fs::read_to_string(&found[0]).unwrap()).unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["summary"], "finished the task", "{result}");
    assert!(pending(&h).is_empty(), "{:?}", pending(&h));
}

#[test]
fn ledger_done_killed_during_the_wake_push_has_recorded_the_report() {
    let h = world("ldone-kill");
    let mut wrapper = bound_task(&h);
    let first = wrapper.run(&h, &as_worker(&h, &["ledger", "done", "finished the task"]));
    assert!(first.status.success(), "{}", text(&first));
    let retry = wrapper.run(&h, &as_worker(&h, &["ledger", "done", "finished the task"]));
    assert!(!retry.status.success(), "{}", text(&retry));
    assert!(
        String::from_utf8_lossy(&retry.stderr).contains("is written 1 time"),
        "{}",
        text(&retry)
    );
    let found = results(&h);
    assert_eq!(found.len(), 1, "{found:?}");
    let result: Value = serde_json::from_str(&std::fs::read_to_string(&found[0]).unwrap()).unwrap();
    assert_eq!(result["summary"], "finished the task", "{result}");
    assert!(pending(&h).is_empty(), "{:?}", pending(&h));
}

#[test]
fn done_retried_after_a_kill_during_the_push_records_one_done() {
    let h = world("done-kill");
    let mut first = as_worker(&h, &["done", "finished the work"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for("the record to be done", || {
        (done_entries(&h) == 1).then_some(())
    });
    assert!(first.try_wait().unwrap().is_none(), "done is still pushing");
    first.kill().unwrap();
    first.wait().unwrap();
    assert!(
        !worker_pane_closed(&h),
        "the killed done left its pane open"
    );
    let lock_path = h.state.join(format!("mail/{WORKSPACE}/courier.lock"));
    wait_for("a courier for the pending report", || {
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&lock_path)
            .ok()?;
        if lock.try_lock().is_err() {
            return Some(());
        }
        let _ = lock.unlock();
        None
    });

    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("an earlier horch done recorded this summary"),
        "{}",
        text(&o)
    );
    assert_eq!(done_entries(&h), 1, "{}", record(&h));
    assert_eq!(pending(&h).len(), 1, "{:?}", pending(&h));
    assert!(worker_pane_closed(&h), "the retry closes the pane");
}

#[test]
fn done_returns_in_time_when_herdr_hangs_on_pane_get() {
    let mut h = world("done-hung-get");
    h.unset("HORCH_WORKSPACE_ID");
    short_budget(&mut h);
    let fake = std::fs::canonicalize(h.bin.join("herdr")).unwrap();
    let flag = h.root.join("hang-pane-get");
    std::fs::write(&flag, "").unwrap();
    let stub = format!(
        "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nif [ \"$1\" = pane ] && [ \"$2\" = get ] && [ -f '{}' ]; then exec /bin/sleep 60; fi\nexec '{}' \"$@\"\n",
        flag.display(),
        fake.display()
    );
    h.write_warmed("herdr", stub.as_bytes());
    let started = Instant::now();
    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    let took = started.elapsed();
    std::fs::remove_file(&flag).unwrap();
    assert_bounded("done", took, &o);
}
