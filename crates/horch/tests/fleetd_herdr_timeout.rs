#![cfg(unix)]

mod support;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util, Store};
use support::fleetd::short_tempdir;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PASS_WAIT: Duration = Duration::from_secs(120);

fn fake_herdr(root: &Path, agent: u32) -> PathBuf {
    let bin = root.join("herdr");
    let calls = root.join("calls.log");
    let sleeper = root.join("sleeper.pid");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> '{calls}'\n\
         case \"$*\" in\n\
         'workspace list') echo '{{\"result\":{{\"workspaces\":[{{\"workspace_id\":\"w1\",\"label\":\"proj\"}}]}}}}' ;;\n\
         'pane list --workspace w1') echo '{{\"result\":{{\"panes\":[{{\"pane_id\":\"p1\",\"workspace_id\":\"w1\"}}]}}}}' ;;\n\
         'pane process-info --pane p1') echo '{{\"result\":{{\"process_info\":{{\"shell_pid\":{agent}}}}}}}' ;;\n\
         'pane get p1') echo '{{\"result\":{{\"pane\":{{\"pane_id\":\"p1\",\"workspace_id\":\"w1\",\"agent_status\":\"working\",\"state_change_seq\":7}}}}}}' ;;\n\
         'pane read p1 '*) echo $$ > '{sleeper}'; exec sleep 900 ;;\n\
         warm) ;;\n\
         *) exit 1 ;;\n\
         esac\n",
        calls = calls.display(),
        sleeper = sleeper.display(),
    );
    std::fs::write(&bin, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Command::new(&bin).arg("warm").status().unwrap().success());
    std::fs::remove_file(&calls).unwrap();
    bin
}

fn git(proj: &Path, home: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(proj)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
        .env("HOME", home)
        .env("GIT_AUTHOR_NAME", "n")
        .env("GIT_AUTHOR_EMAIL", "a@b")
        .env("GIT_COMMITTER_NAME", "n")
        .env("GIT_COMMITTER_EMAIL", "a@b")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn seed_ledger(root: &Path) -> PathBuf {
    let proj = root.join("proj");
    let home = root.join("home");
    std::fs::create_dir_all(&proj).unwrap();
    git(&proj, &home, &["init", "-q", "-b", "main"]);
    git(
        &proj,
        &home,
        &["commit", "-q", "--allow-empty", "-m", "init"],
    );
    git(&proj, &home, &["branch", "fleet/r1", "main"]);
    let text = "# Ticket One\n";
    std::fs::write(proj.join("t1.md"), text).unwrap();
    std::fs::write(root.join("machine.json"), HEALTHY_MACHINE).unwrap();
    std::fs::create_dir_all(root.join("fleet")).unwrap();
    std::fs::write(
        root.join("fleet/fleet.toml"),
        "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
    )
    .unwrap();
    let db = home.join(".horch/ledger.sqlite3");
    let new = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some("fleet/r1".into()),
        thresholds: Thresholds {
            check_every_s: Some(10),
            stall_rounds: Some(1000),
            ..Default::default()
        },
        ..Default::default()
    };
    let facts = ticket_facts(text);
    let mut task = NewTask::work(&facts.title.clone().unwrap(), "t1.md", "dev");
    task.ticket_snapshot = Some(text.into());
    let project = proj.canonicalize().unwrap();
    let mut store = Store::open(&db).unwrap();
    store
        .immediate(|tx| {
            let run = runs::create(tx, &project, &new)?;
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            tasks::add(tx, &run, &task, Actor::Orchestrator)?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let worker = NewWorker {
        persona: "dev".into(),
        role: Some("dev1".into()),
        workspace_id: Some("w1".into()),
        pane_id: Some("p1".into()),
        ..Default::default()
    };
    support::bind::bind(&db, worker, Some(std::process::id()));
    db
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the pid exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn sleeper(root: &Path) -> Option<i32> {
    std::fs::read_to_string(root.join("sleeper.pid"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

#[test]
fn a_herdr_read_that_never_answers_does_not_hold_the_pass() {
    let tmp = short_tempdir();
    let root = tmp.path();
    let db = seed_ledger(root);
    let herdr = fake_herdr(root, std::process::id());
    let mut child = Command::new(env!("CARGO_BIN_EXE_horch"))
        .args(["ledger", "watch", "--once", "--force"])
        .current_dir(root.join("proj"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_PROJECT_DIR", root.join("proj"))
        .env("HORCH_MACHINE_FILE", root.join("machine.json"))
        .env("HORCH_HERDR_BIN", &herdr)
        .env("FLEET_ROOT", root.join("fleet"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let began = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if began.elapsed() > PASS_WAIT {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if status.is_none() {
        // SAFETY: the pass runs in its own process group; the hung read is
        // its child.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
            if let Some(pid) = sleeper(root) {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        let _ = child.wait();
        panic!("the pass did not end in {PASS_WAIT:?}: a herdr read with no answer holds it");
    }
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(status.unwrap().success(), "stderr:\n{err}");

    let calls = std::fs::read_to_string(root.join("calls.log")).unwrap();
    assert!(
        calls.contains("pane read p1"),
        "the pass read the pane:\n{calls}"
    );
    assert!(!calls.contains("pane close"), "no pane is closed:\n{calls}");
    let pid = sleeper(root).expect("the hung read wrote its pid");
    assert!(!alive(pid), "the hung read {pid} is killed at its limit");

    let heartbeat = root.join("fleet/run/heartbeat.json");
    let written = std::fs::metadata(&heartbeat)
        .unwrap_or_else(|e| panic!("no heartbeat {} ({e}); stderr:\n{err}", heartbeat.display()))
        .modified()
        .unwrap();
    assert!(
        written.elapsed().unwrap() <= began.elapsed(),
        "the pass wrote its heartbeat"
    );

    let store = Store::open(&db).unwrap();
    let state: String = store
        .conn()
        .query_row("SELECT state FROM attempts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "live", "no attempt is stopped; stderr:\n{err}");
}
