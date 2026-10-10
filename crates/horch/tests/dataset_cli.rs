//! The `multi-herdr-dataset` command line (CMP-01) and the `outcome`
//! state check (EXP-06).

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Duration, TimeZone, Utc};
use clap::Parser;
use horch::dataset::cli::{Cli, Command as DatasetCommand, OutcomeArg};
use horch::dataset::outcome::check_round;
use horch::dataset::status::render;
use horch::dataset::{resolve_project, spelled_as, target_line};
use horch_core::competition::config::{parse_usd_micro, JudgeMode, Strategy};
use horch_core::competition::model::RoundState;
use horch_core::competition::preflight::PreflightReport;
use horch_core::harness::HarnessKind;
use horch_core::ids::{
    ExecutionId, ExperimentId, JudgmentId, ModelId, RoundId, TaskId, TeammateName,
};
use horch_core::measure::digest::sha256_bytes;
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::fold;
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::{self, StoreOptions};
use horch_core::teacher::TeacherRef;
use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_multi-herdr-dataset");

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("multi-herdr-dataset").chain(args.iter().copied()))
}

#[test]
fn cmp_01_cli_args() {
    let cli = parse(&[
        "run",
        "fix the parser",
        "--candidates",
        "4",
        "--strategy",
        "diverse",
        "--budget-usd",
        "12.345678",
        "--judge",
        "auto",
        "--baseline",
        "sonnet",
        "--promote-to",
        "main",
        "--worktree-root",
        "/tmp/wt",
        "--allow-dirty",
    ])
    .unwrap();
    let DatasetCommand::Run(args) = cli.command else {
        panic!("not run");
    };
    let flags = args.flags().unwrap();
    assert_eq!(flags.task, "fix the parser");
    assert_eq!(flags.candidates, Some(4));
    assert_eq!(flags.strategy, Some(Strategy::Diverse));
    assert_eq!(flags.judge, Some(JudgeMode::Auto));
    assert_eq!(flags.baseline, Some(TeammateName::new("sonnet").unwrap()));
    assert_eq!(flags.promote_to.as_deref(), Some("main"));
    assert_eq!(flags.worktree_root.as_deref(), Some(Path::new("/tmp/wt")));
    assert!(flags.allow_dirty);
    // `--budget-usd` is kept as typed and parsed digit by digit: exact.
    assert_eq!(flags.budget_usd.as_deref(), Some("12.345678"));
    assert_eq!(
        parse_usd_micro(flags.budget_usd.as_deref().unwrap()).unwrap(),
        12_345_678
    );

    // Without flags, nothing overrides the config file.
    let DatasetCommand::Run(bare) = parse(&["run", "t"]).unwrap().command else {
        panic!("not run");
    };
    let flags = bare.flags().unwrap();
    assert_eq!(
        (flags.candidates, flags.budget_usd, flags.allow_dirty),
        (None, None, false)
    );

    // A task is required; a bad strategy or judge mode is refused.
    assert!(parse(&["run"]).is_err());
    assert!(parse(&["run", "t", "--strategy", "random"]).is_err());
    assert!(parse(&["run", "t", "--judge", "manual"]).is_err());
    assert!(parse(&["run", "t", "--candidates", "two"]).is_err());

    // The data commands.
    assert!(matches!(
        parse(&["status"]).unwrap().command,
        DatasetCommand::Status { experiment: None }
    ));
    assert!(matches!(
        parse(&["export", "--label-policy", "slot-order-1"])
            .unwrap()
            .command,
        DatasetCommand::Export {
            label_policy: Some(_)
        }
    ));
    assert!(matches!(
        parse(&["readiness", "--policy", "p.json"]).unwrap().command,
        DatasetCommand::Readiness {
            policy: Some(_),
            ..
        }
    ));
    let DatasetCommand::Outcome {
        round,
        kind,
        score,
        note,
    } = parse(&[
        "outcome",
        "r1",
        "--kind",
        "revert",
        "--score",
        "0.25",
        "--note",
        "rolled back",
    ])
    .unwrap()
    .command
    else {
        panic!("not outcome");
    };
    assert_eq!(
        (round.as_str(), kind, score, note.as_deref()),
        ("r1", OutcomeArg::Revert, Some(0.25), Some("rolled back"))
    );
    assert!(parse(&["outcome", "r1", "--kind", "great"]).is_err());
    assert!(parse(&["outcome", "r1"]).is_err());
    assert!(matches!(
        parse(&["rebuild", "e1"]).unwrap().command,
        DatasetCommand::Rebuild { .. }
    ));

    // `--help` lists every public subcommand and hides the placeholders.
    let out = Command::new(BIN).arg("--help").output().unwrap();
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for sub in ["run", "status", "export", "readiness", "outcome", "rebuild"] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(sub)),
            "{sub}: {help}"
        );
    }
    for hidden in ["judge-job", "promote", "rollback", "cleanup", "watch"] {
        assert!(!help.contains(hidden), "{hidden}: {help}");
    }

    // The operator commands refuse a round that does not exist (exit 1).
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        &["promote", "r1", "--to", "main"][..],
        &["rollback", "r1"],
        &["cleanup", "r1"],
    ] {
        let out = Command::new(BIN)
            .args(args)
            .current_dir(tmp.path())
            .env_remove("ANTHROPIC_API_KEY")
            .env("HORCH_STATE_DIR", tmp.path().join("state"))
            .env("HORCH_PROJECT_DIR", tmp.path())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(stderr.contains("no such round r1"), "{args:?}: {stderr}");
        assert!(!stderr.contains("not implemented"), "{args:?}: {stderr}");
    }
}

// ─── EXP-06 ─────────────────────────────────────────────────────────────────

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap()
}

fn uuid(n: u64) -> String {
    format!("0199a5b0-0000-7000-8000-{n:012x}")
}

/// One experiment with a passed preflight and one round of 2 planned
/// candidates: the round is PROVISIONING.
fn provisioning_round(paths: &DatasetPaths) -> (ExperimentId, RoundId) {
    let exp = ExperimentId::new(uuid(0xe01)).unwrap();
    let round = RoundId::new(uuid(0xa01)).unwrap();
    let rec = JsonlRecorder::open(paths, StoreOptions::default()).unwrap();
    let report: PreflightReport = serde_json::from_value(json!({
        "schema_version": "1.0.0",
        "checks": [{"id": "PRE-01", "status": "pass", "detail": "git ok", "measured": null}],
        "safe_n": 2,
        "waves": 1,
        "projected_cost_microusd": 1_500_000,
        "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                    "mem_total_bytes": null, "mem_available_bytes": null,
                    "disk_free_bytes": null, "disk_total_bytes": null,
                    "gpu": "apple_silicon", "max_open_files": null, "max_processes": null},
        "environment_digest": sha256_bytes(b"env").to_string(),
        "passed": true,
    }))
    .unwrap();
    let mut n = 0;
    let mut push = |kind: EventKind, round_id: Option<RoundId>, execution: Option<ExecutionId>| {
        n += 1;
        rec.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: exp.clone(),
            round_id,
            execution_id: execution,
            idempotency_key: format!("k{n}"),
            occurred_at: t0() + Duration::seconds(n),
        })
        .unwrap();
    };
    push(
        EventKind::ExperimentCreated(ExperimentCreated {
            task_id: TaskId::new("task-1").unwrap(),
            task_digest: sha256_bytes(b"task"),
            config_digest: sha256_bytes(b"config"),
            base_sha: "a".repeat(40),
            repo_digest: sha256_bytes(b"repo"),
            environment_digest: sha256_bytes(b"env"),
            candidates: 2,
            strategy: "diverse".into(),
            budget_usd_micro: 5_000_000,
            promote_to: None,
        }),
        None,
        None,
    );
    push(
        EventKind::PreflightCompleted(PreflightCompleted { report }),
        None,
        None,
    );
    push(
        EventKind::RoundCreated(RoundCreated {
            index: 0,
            base_sha: "a".repeat(40),
            labels: vec!["A".into(), "B".into()],
            eligible_set: Vec::new(),
            propensities: [("A".to_string(), 1.0), ("B".to_string(), 1.0)].into(),
            teacher: TeacherRef::none(),
            seed: 1,
            label_policy_version: "slot-order-1".into(),
        }),
        Some(round.clone()),
        None,
    );
    for (i, label) in ["A", "B"].into_iter().enumerate() {
        push(
            EventKind::CandidatePlanned(CandidatePlanned {
                label: label.into(),
                teammate: TeammateName::new("sonnet").unwrap(),
                harness: HarnessKind::Claude,
                model: ModelId::new("sonnet").unwrap(),
                effort: None,
                slot: SlotKind::Diversity,
                propensity: 1.0,
                config_id: format!("sonnet-{label}|claude|sonnet|-"),
            }),
            Some(round.clone()),
            Some(ExecutionId::new(uuid(0xc00 + i as u64)).unwrap()),
        );
    }
    (exp, round)
}

#[test]
fn exp_06_outcome_cli_refuses_wrong_state() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&project).unwrap();
    let paths = DatasetPaths::new(&state, &project);
    let (_exp, round) = provisioning_round(&paths);
    let before = store::read_all(&paths).unwrap().events.len();

    let outcome = |round: &str| {
        Command::new(BIN)
            .args(["outcome", round, "--kind", "verified"])
            .current_dir(&project)
            .env_remove("ANTHROPIC_API_KEY")
            .env("HORCH_PROJECT_DIR", &project)
            .env("HORCH_STATE_DIR", &state)
            .output()
            .unwrap()
    };

    // A round that is still provisioning has no winner: refused.
    let out = outcome(round.as_str());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("PROVISIONING"), "{stderr}");
    // An unknown round: refused.
    let out = outcome(&uuid(0xa99));
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no round"));
    // Nothing was appended.
    assert_eq!(store::read_all(&paths).unwrap().events.len(), before);

    // The state rule itself, on the folded view.
    let view = fold(&store::read_all(&paths).unwrap().events).rounds[&round].clone();
    let winner = WinnerSelected {
        label: "A".into(),
        execution_id: ExecutionId::new(uuid(0xc00)).unwrap(),
        head_sha: "b".repeat(40),
        judgment_id: JudgmentId::new(uuid(0xd01)).unwrap(),
        promotion: serde_json::from_value(json!("not_requested")).unwrap(),
    };
    for state in [
        RoundState::Running,
        RoundState::JudgingBackground,
        RoundState::Rejected,
        RoundState::NeedsIntervention,
        RoundState::Cleanup,
    ] {
        let mut v = view.clone();
        v.state = state;
        v.winner = Some(winner.clone());
        assert!(check_round(&round, &v).is_err(), "{state:?}");
    }
    for state in [
        RoundState::Decided,
        RoundState::Promoted,
        RoundState::Complete,
    ] {
        let mut v = view.clone();
        v.state = state;
        assert!(
            check_round(&round, &v).is_err(),
            "{state:?} without a winner"
        );
        v.winner = Some(winner.clone());
        assert!(check_round(&round, &v).is_ok(), "{state:?} with a winner");
    }
}

// ─── F3: which repo a command targets ───────────────────────────────────────

/// `git` in `dir` with a fixed identity; panics on failure.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `status` in `cwd` with `HORCH_PROJECT_DIR=env_project`: its first line.
fn status_target(cwd: &Path, env_project: &Path, state: &Path, extra: &[&str]) -> String {
    let out = Command::new(BIN)
        .args(extra)
        .arg("status")
        .current_dir(cwd)
        .env_remove("ANTHROPIC_API_KEY")
        .env("HORCH_PROJECT_DIR", env_project)
        .env("HORCH_STATE_DIR", state)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}{out:?}");
    stdout.lines().next().unwrap_or_default().to_string()
}

#[test]
fn f3_project_precedence_and_target_line() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    let fleet = root.join("fleet");
    let plain = root.join("plain");
    let state = root.join("state");
    for dir in [&repo, &fleet, &plain] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(repo.join("src")).unwrap();

    // The cwd's repo beats `HORCH_PROJECT_DIR`, from a subdirectory too.
    let want = format!("target: {} @ {}", repo.display(), &head[..12]);
    assert_eq!(status_target(&repo, &fleet, &state, &[]), want);
    assert_eq!(status_target(&repo.join("src"), &fleet, &state, &[]), want);
    // `--project` beats both, before or after the subcommand.
    let flag = fleet.to_string_lossy().into_owned();
    assert_eq!(
        status_target(&repo, &plain, &state, &["--project", &flag]),
        format!("target: {} @ no HEAD", fleet.display())
    );
    // A cwd in no git repo falls back to `HORCH_PROJECT_DIR`.
    assert_eq!(status_target(&plain, &repo, &state, &[]), want);

    // The rule itself: a relative flag is relative to the cwd.
    let top = |_: &Path| Some(PathBuf::from("/top"));
    let none = |_: &Path| None;
    let cwd = Some(Path::new("/cwd"));
    let env = Some(Path::new("/env"));
    assert_eq!(
        resolve_project(Some(Path::new("p")), cwd, env, top),
        Some(PathBuf::from("/cwd/p"))
    );
    assert_eq!(
        resolve_project(None, cwd, env, top),
        Some(PathBuf::from("/top"))
    );
    assert_eq!(
        resolve_project(None, cwd, env, none),
        Some(PathBuf::from("/env"))
    );
    assert_eq!(resolve_project(None, None, None, top), None);

    // The top level keeps the cwd's spelling through a symlink.
    #[cfg(unix)]
    {
        let link = root.join("link");
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        assert_eq!(spelled_as(&repo, &link.join("src")), link);
        assert_eq!(spelled_as(&repo, &link), link);
        assert_eq!(spelled_as(&repo, &plain), repo, "unrelated dir");
    }
    assert_eq!(
        target_line(Path::new("/r"), Some(&"c".repeat(40))),
        format!("target: /r @ {}", "c".repeat(12))
    );
}

// ─── F6: the experiment state `status` shows ────────────────────────────────

#[test]
fn f6_status_shows_the_experiment_at_its_round_state() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&project).unwrap();
    let paths = DatasetPaths::new(&state, &project);
    let (exp, round) = provisioning_round(&paths);

    // The live command: the round is PROVISIONING, so is the experiment.
    let out = Command::new(BIN)
        .arg("status")
        .current_dir(&project)
        .env_remove("ANTHROPIC_API_KEY")
        .env("HORCH_PROJECT_DIR", &project)
        .env("HORCH_STATE_DIR", &state)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(
        stdout.contains(&format!("experiment {exp} PROVISIONING ")),
        "{stdout}"
    );

    // While the round runs, and after it is COMPLETE.
    let projection = fold(&store::read_all(&paths).unwrap().events);
    assert_eq!(projection.experiments[&exp].state, RoundState::Planned);
    for state in [RoundState::Running, RoundState::Complete] {
        let mut p = projection.clone();
        p.rounds.get_mut(&round).unwrap().state = state;
        let text = render(&p, None, 0);
        assert!(
            text.contains(&format!("experiment {exp} {} ", state.as_str())),
            "{state:?}: {text}"
        );
    }
}
