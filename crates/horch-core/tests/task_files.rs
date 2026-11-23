use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::new_features::{open_feature, Project};
use horch_core::fleetd::task_execution::adapters::task_files::{
    attempt_dir, write_attempt_files, TaskFilesError,
};
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, write_task_files, DispatchCtx,
};
use horch_core::fleetd::task_execution::application::planned_tasks::{
    add_planned_tasks, PlannedTask,
};
use horch_core::fleetd::task_execution::domain::task_files::{Secret, Sr2Rule, TaskFacts};
use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;

const SNAPSHOT: &str = "# 07: a ticket\n\n**Check:** `cargo test`\n\nDo the work.\n";

fn facts(root: &Path) -> TaskFacts {
    TaskFacts {
        task: "T7".into(),
        task_id: "0b5f9c1e-6d8a-4c1b-9a51-1f1f0e2d3c4b".into(),
        run: "fleetd-build".into(),
        attempt: "A2".into(),
        profile: "opus".into(),
        worktree: root.join("work").join("T7").to_string_lossy().into_owned(),
        branch: "fleet/fleetd-build-T7-task-files".into(),
        read_first: "docs/tickets/fleetd-p4/07-task-yaml-and-start-md.md".into(),
        goal: "task.yaml and START.md: fleetd writes them once".into(),
        check: Some("cargo test -p horch-core --test task_files && echo \"ok\"".into()),
        kind: Some("feature".into()),
        ticket_snapshot: SNAPSHOT.into(),
        extra: None,
        handoff: None,
        idea: None,
        feature: None,
        covers: Vec::new(),
        verdict: None,
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn task_yaml_has_each_key_and_1_value_per_line() {
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let files = write_attempt_files(root.path(), &f, &[]).unwrap();
    let dir = attempt_dir(root.path(), &f.task_id, "A2");
    assert_eq!(files.task_yaml, dir.join("task.yaml"));
    let text = read(&files.task_yaml);

    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.contains(&"attempt: A2"), "{text}");
    assert!(
        lines.contains(&format!("worktree: {}", f.worktree).as_str()),
        "{text}"
    );

    let keys: Vec<&str> = text.lines().map(|l| l.split_once(':').unwrap().0).collect();
    assert_eq!(
        keys,
        [
            "task",
            "idea",
            "task_id",
            "run",
            "attempt",
            "profile",
            "worktree",
            "branch",
            "read_first",
            "goal",
            "covers",
            "check",
            "kind",
            "lease",
            "report_to"
        ],
        "1 key on each line, in the order of the design: {text}"
    );

    let y: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
    assert_eq!(y["task"], "T7");
    assert_eq!(y["task_id"], f.task_id.as_str());
    assert_eq!(y["run"], "fleetd-build");
    assert_eq!(y["attempt"], "A2");
    assert_eq!(y["profile"], "opus");
    assert_eq!(y["worktree"], f.worktree.as_str());
    assert_eq!(y["branch"], f.branch.as_str());
    assert_eq!(y["read_first"][0], f.read_first.as_str());
    assert_eq!(y["goal"], f.goal.as_str(), "a colon in the goal is quoted");
    assert_eq!(
        y["check"],
        f.check.as_deref().unwrap(),
        "quotes in the check survive"
    );
    assert_eq!(y["kind"], "feature");
    assert_eq!(y["lease"]["renew_every_s"], 10);
    let outbox = dir.join("outbox");
    assert_eq!(y["report_to"], format!("{}/", outbox.display()).as_str());

    let root2 = tempfile::tempdir().unwrap();
    let mut bare = facts(root2.path());
    bare.check = None;
    bare.kind = None;
    let files = write_attempt_files(root2.path(), &bare, &[]).unwrap();
    let y: serde_yaml::Value = serde_yaml::from_str(&read(&files.task_yaml)).unwrap();
    assert!(y["check"].is_null());
    assert!(y["kind"].is_null());
}

#[test]
fn start_md_holds_the_ticket_and_the_result_line() {
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let files = write_attempt_files(root.path(), &f, &[]).unwrap();
    let dir = attempt_dir(root.path(), &f.task_id, "A2");
    assert_eq!(files.start_md, dir.join("inbox").join("START.md"));
    let text = read(&files.start_md);
    assert!(
        text.starts_with(SNAPSHOT),
        "the ticket snapshot comes first: {text}"
    );
    assert!(text.contains(&f.worktree), "{text}");
    assert!(text.contains(&f.branch), "{text}");
    assert!(
        text.contains(&dir.join("outbox").display().to_string()),
        "{text}"
    );
    assert!(
        text.contains("When the work is committed, run `horch result`."),
        "{text}"
    );
    assert!(
        text.contains("To ask a question, send `QUESTION:`."),
        "{text}"
    );

    let root2 = tempfile::tempdir().unwrap();
    let mut fix = facts(root2.path());
    fix.extra = Some("## Conflict\n\nResolve src/a.rs.\n".into());
    let text = read(
        &write_attempt_files(root2.path(), &fix, &[])
            .unwrap()
            .start_md,
    );
    let fixed = text.find("horch result").unwrap();
    let extra = text.find("## Conflict").unwrap();
    assert!(fixed < extra, "{text}");
}

#[test]
fn existing_file_is_never_rewritten() {
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let first = write_attempt_files(root.path(), &f, &[]).unwrap();
    let yaml = read(&first.task_yaml);
    let start = read(&first.start_md);

    let again = write_attempt_files(root.path(), &f, &[]).unwrap();
    assert_eq!(again, first);
    assert_eq!(read(&first.task_yaml), yaml);
    assert_eq!(read(&first.start_md), start);

    let mut changed = facts(root.path());
    changed.goal = "another goal".into();
    changed.ticket_snapshot = "# another ticket\n".into();
    let err = write_attempt_files(root.path(), &changed, &[]).unwrap_err();
    assert!(matches!(err, TaskFilesError::Exists(_)), "{err:?}");
    assert_eq!(read(&first.task_yaml), yaml);
    assert_eq!(read(&first.start_md), start);
}

#[test]
fn half_written_file_is_never_read() {
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let dir = attempt_dir(root.path(), &f.task_id, "A2");

    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    std::fs::write(dir.join("task.yaml.tmp"), "task: T7\natt").unwrap();
    std::fs::write(dir.join("inbox").join("START.md.tmp"), "# 07").unwrap();

    let files = write_attempt_files(root.path(), &f, &[]).unwrap();
    let y: serde_yaml::Value = serde_yaml::from_str(&read(&files.task_yaml)).unwrap();
    assert!(!y["report_to"].as_str().unwrap().is_empty());
    assert!(read(&files.start_md).starts_with(SNAPSHOT));

    assert_eq!(names(&dir), ["inbox", "task.yaml", "task.yaml.tmp"]);
    assert_eq!(names(&dir.join("inbox")), ["START.md", "START.md.tmp"]);
}

fn no_file_written(root: &Path, f: &TaskFacts) {
    let dir: PathBuf = attempt_dir(root, &f.task_id, &f.attempt);
    assert!(!dir.join("task.yaml").exists());
    assert!(!dir.join("inbox").join("START.md").exists());
    if dir.exists() {
        let leftovers: Vec<String> = walk(&dir);
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p.display().to_string());
        }
    }
    out
}

#[test]
fn sr2_profile_env_value_in_a_task_file_is_refused() {
    let value = "ghp_s3cr3tV4lue0fTheProfile";
    let profile = [Secret::profile("GH_TOKEN", value)];

    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.ticket_snapshot = format!("{SNAPSHOT}Use {value} to push.\n");
    let err = write_attempt_files(root.path(), &f, &profile).unwrap_err();
    let TaskFilesError::Secret(refusal) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(refusal.rule, Sr2Rule::ProfileEnvValue);
    assert_eq!(refusal.file, "inbox/START.md");
    let shown = err.to_string();
    assert!(
        shown.contains("SR-2") && shown.contains("GH_TOKEN"),
        "{shown}"
    );
    assert!(
        !shown.contains(value),
        "the error never shows the value: {shown}"
    );
    no_file_written(root.path(), &f);

    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.check = Some(format!("GH=\"{value}\" cargo test"));
    let err = write_attempt_files(root.path(), &f, &profile).unwrap_err();
    let TaskFilesError::Secret(refusal) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(refusal.file, "task.yaml");
    no_file_written(root.path(), &f);

    for line in [
        "export DEPLOY_KEY=abc",
        "GITHUB_TOKEN=xyz",
        "  APP_SECRET=1",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut f = facts(root.path());
        f.ticket_snapshot = format!("{SNAPSHOT}{line}\n");
        let err = write_attempt_files(root.path(), &f, &[]).unwrap_err();
        let TaskFilesError::Secret(refusal) = &err else {
            panic!("{line}: {err:?}")
        };
        assert_eq!(refusal.rule, Sr2Rule::SecretAssignment, "{line}");
        assert!(!err.to_string().contains("abc"), "{err}");
        no_file_written(root.path(), &f);
    }

    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.ticket_snapshot = format!("{SNAPSHOT}Set GH_TOKEN in the profile. Use 1 worker.\n");
    let short = [Secret::profile("ONE", "1")];
    write_attempt_files(root.path(), &f, &short).unwrap();
}

#[test]
fn sr2_forbidden_env_name_in_a_task_file_is_refused() {
    let name = FORBIDDEN_ENV[0];
    let value = "fake-forbidden-value-0123456789";
    let secrets = [Secret::forbidden(name, value)];

    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.goal = format!("Log in with {value}");
    let err = write_attempt_files(root.path(), &f, &secrets).unwrap_err();
    let TaskFilesError::Secret(refusal) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(refusal.rule, Sr2Rule::ForbiddenEnvValue);
    assert_eq!(refusal.file, "task.yaml");
    let shown = err.to_string();
    assert!(shown.contains(name), "{shown}");
    assert!(!shown.contains(value), "{shown}");
    no_file_written(root.path(), &f);

    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.ticket_snapshot = format!("{SNAPSHOT}Never use {name}.\n");
    write_attempt_files(root.path(), &f, &secrets).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    let out = cmd
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn planned(key: &str, kind: &str) -> PlannedTask {
    PlannedTask {
        key: key.into(),
        title: format!("The {kind} task"),
        task_file: "/fleet/ideas/plan.yaml".into(),
        ticket_snapshot: Some(format!("# {key}\n")),
        work_kind: None,
        profile: "opus".into(),
        land_check: Some("true".into()),
        spec_json: serde_json::json!({"key": key, "kind": kind}).to_string(),
        after: Vec::new(),
        judges: None,
        own: Vec::new(),
    }
}

#[test]
fn planned_non_legacy_kinds_survive_dispatch_into_task_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let mut store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    let config = FleetConfig::default();
    let jobs = || JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let cause = |text| Cause {
        actor: Actor::Horch,
        text,
    };

    let project = Project {
        root: &repo,
        primary_branch: "main",
    };
    let feature = store
        .immediate(|tx| open_feature(tx, &jobs(), &project, cause("idea")))
        .unwrap();
    git(
        &repo,
        &[
            "branch",
            &format!("fleet/{}", feature.to_lowercase()),
            "main",
        ],
    );
    store
        .immediate(|tx| {
            let new = NewIdea {
                id: "I-1",
                feature_id: &feature,
                priority: 0,
            };
            ideas_store::insert_idea(tx, None, &new, cause("idea added"))
        })
        .unwrap();
    let run = runs::of_feature(store.conn(), &feature).unwrap().unwrap();

    let kinds = ["judge", "research", "design", "creative"];
    let plan: Vec<PlannedTask> = kinds
        .iter()
        .map(|kind| planned(&format!("{kind}-task"), kind))
        .collect();
    let added = store
        .immediate(|tx| add_planned_tasks(tx, None, &run, "I-1", &plan, cause("plan")))
        .unwrap();
    assert_eq!(added.len(), kinds.len());
    for (key, task_id) in &added {
        let work_kind: Option<String> = store
            .conn()
            .query_row(
                "SELECT work_kind FROM tasks WHERE task_id = ?1",
                [task_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(work_kind, None, "{key}: the kind lives only in spec_json");
    }

    let git_port = Git::new("git");
    let ctx = DispatchCtx {
        jobs: jobs(),
        clock: FleetSeconds(1_000),
        git: &git_port,
    };
    let fleet_root = root.join("fleet");
    for (kind, (key, task_id)) in kinds.iter().zip(&added) {
        let bound = store
            .immediate(|tx| bind(tx, &ctx, task_id, "opus", &NewWorker::default()))
            .unwrap();
        let files = write_task_files(store.conn(), &fleet_root, &bound.attempt_id, &[]).unwrap();
        let text = read(&files.task_yaml);
        let y: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(y["kind"], *kind, "{key}: {text}");
        assert!(
            !text.lines().any(|l| l.trim() == "kind: null"),
            "{key}: {text}"
        );
    }
}
