//! B4 judge job, coordinator side: discovery of a job after a restart
//! (JDG-08), the single-authority judgment write (JDG-04), the terminal-set
//! rule (CMP-12) and the judge execution record (JDG-09). Dataset design
//! §4.7 and §5.1.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use horch_core::evaluation::scheduler::{
    decide, discover, ExitReason, Heartbeat, JobExit, JobFacts, JobState, EXIT_FILE,
    HEARTBEAT_FILE, LOG_FILE, OUTPUT_FILE,
};

fn at(s: &str) -> DateTime<Utc> {
    horch_core::clock::parse(s).unwrap()
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

/// JDG-08: after a restart the coordinator reads the job dir. Output present
/// → parse; heartbeat fresh and pid alive → wait; otherwise lost.
#[test]
fn jdg_08_restart_discovers_job() {
    let now = at("2026-10-02T12:00:30Z");
    let stale = Duration::from_secs(30);
    let fresh_hb = |pid| Heartbeat {
        pid,
        at: "2026-10-02T12:00:25Z".into(),
    };
    let old_hb = Heartbeat {
        pid: 7,
        at: "2026-10-02T11:59:00Z".into(),
    };
    let crash = JobExit {
        reason: ExitReason::Crash,
        code: Some(3),
    };
    let rows: Vec<(&str, JobFacts, JobState)> = vec![
        ("nothing spawned", JobFacts::default(), JobState::NotStarted),
        (
            "spawned, no heartbeat yet",
            JobFacts {
                spawned_at: Some(at("2026-10-02T12:00:20Z")),
                ..JobFacts::default()
            },
            JobState::Running { pid: 0 },
        ),
        (
            "spawned long ago, never beat",
            JobFacts {
                spawned_at: Some(at("2026-10-02T11:00:00Z")),
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "fresh heartbeat, pid alive",
            JobFacts {
                heartbeat: Some(fresh_hb(42)),
                pid_alive: true,
                spawned_at: Some(at("2026-10-02T11:00:00Z")),
                ..JobFacts::default()
            },
            JobState::Running { pid: 42 },
        ),
        (
            "fresh heartbeat, pid dead",
            JobFacts {
                heartbeat: Some(fresh_hb(42)),
                pid_alive: false,
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "stale heartbeat, pid alive",
            JobFacts {
                heartbeat: Some(old_hb.clone()),
                pid_alive: true,
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "output present, job gone",
            JobFacts {
                output: true,
                heartbeat: Some(old_hb.clone()),
                ..JobFacts::default()
            },
            JobState::OutputPresent(OUTPUT_FILE.into()),
        ),
        (
            "output and exit: output wins",
            JobFacts {
                output: true,
                exit: Some(Some(crash.clone())),
                ..JobFacts::default()
            },
            JobState::OutputPresent(OUTPUT_FILE.into()),
        ),
        (
            "exit without output",
            JobFacts {
                exit: Some(Some(crash.clone())),
                heartbeat: Some(fresh_hb(42)),
                pid_alive: true,
                ..JobFacts::default()
            },
            JobState::Exited(crash.clone()),
        ),
        (
            "unreadable exit file is a crash",
            JobFacts {
                exit: Some(None),
                ..JobFacts::default()
            },
            JobState::Exited(JobExit {
                reason: ExitReason::Crash,
                code: None,
            }),
        ),
    ];
    for (name, facts, want) in rows {
        assert_eq!(decide(&facts, now, stale), want, "{name}");
    }

    // The same rules through real files.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    assert_eq!(discover(dir, now, stale), JobState::NotStarted);
    write(dir, LOG_FILE, "");
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Running { pid: 0 }
    );
    let me = std::process::id();
    write(
        dir,
        HEARTBEAT_FILE,
        &serde_json::to_string(&Heartbeat {
            pid: me,
            at: horch_core::clock::stamp(Utc::now()),
        })
        .unwrap(),
    );
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Running { pid: me }
    );
    // An hour later the same heartbeat is stale.
    assert_eq!(
        discover(dir, Utc::now() + chrono::Duration::hours(1), stale),
        JobState::Lost
    );
    write(dir, EXIT_FILE, r#"{"reason":"timeout","code":null}"#);
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Exited(JobExit {
            reason: ExitReason::Timeout,
            code: None
        })
    );
    write(dir, OUTPUT_FILE, "{}");
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::OutputPresent(dir.join(OUTPUT_FILE))
    );
}
