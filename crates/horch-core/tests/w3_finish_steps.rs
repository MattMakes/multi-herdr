//! The finish steps on a real git repository in a temp dir: land, cleanup,
//! complexity-sweep, pr, code-sweep, `finish` and `done` (#137, #138, #141,
//! #144, #146). Each test pins the outcome text of 1 path, so a refactor
//! that changes a path fails here.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::finish::{
    self, codesweep, land, CodexSandbox, Ctx, FinishReport, Step, Tools, WorktreeRow,
};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::worktree_ops;
use horch_core::taskledger::Store;

const RUN_BRANCH: &str = "fleet/r1";
const BRANCH: &str = "fleet/r1-T1-feat";

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Write `file` in `dir`, commit it, and return the new HEAD.
fn commit(dir: &Path, file: &str, body: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A commit on the run branch, made without a checkout.
fn move_run_branch_script(root: &Path) -> String {
    format!(
        "c=$(git -C {r} commit-tree -p {RUN_BRANCH} -m moved {RUN_BRANCH}^{{tree}}) && git -C {r} update-ref refs/heads/{RUN_BRANCH} $c",
        r = root.display()
    )
}

struct World {
    tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    run: Run,
    tools: Tools,
    base: String,
}

impl World {
    fn new(steps: &[StepName], remote: bool) -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", k, v]);
        }
        let base = commit(&root, "README.md", "base\n");
        git(&root, &["branch", RUN_BRANCH]);
        let remote = remote.then(|| {
            let bare = top.join("remote.git");
            git(
                &top,
                &["init", "--quiet", "--bare", &bare.to_string_lossy()],
            );
            git(&root, &["remote", "add", "origin", &bare.to_string_lossy()]);
            git(&root, &["push", "--quiet", "origin", RUN_BRANCH]);
            "origin".to_string()
        });
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        remote,
                        finish_steps: steps.to_vec(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let tools = Tools {
            git: Git::new("git"),
            gh: top.join("gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: top.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
        };
        World {
            tmp,
            root,
            store,
            run,
            tools,
            base,
        }
    }

    fn top(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }

    /// A claimed task of a new `persona` worker; returns the worker id.
    fn claimed(&mut self, persona: &str, f: impl FnOnce(&mut NewTask)) -> String {
        let mut t = NewTask::work("feature", "t.md", persona);
        f(&mut t);
        let run = self.run.clone();
        self.store
            .immediate(|tx| {
                tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                self::worker(tx, &run, persona)
            })
            .unwrap()
    }

    /// A claimed worktree task with `check`, and its worktree with 1 commit
    /// of `file`.
    fn task(&mut self, check: Option<&str>, file: &str) -> (String, PathBuf) {
        let w = self.claimed("opus", |t| t.land_check = check.map(Into::into));
        let wt = finish::worktree_new(&mut self.store, &self.tools.git, &w, "feat").unwrap();
        let path = PathBuf::from(&wt.path);
        commit(&path, file, "work\n");
        (w, path)
    }

    fn finish(&mut self, w: &str) -> FinishReport {
        finish::finish(&mut self.store, &self.tools, w).unwrap()
    }

    fn worktree(&self) -> WorktreeRow {
        let task: String = self
            .store
            .conn()
            .query_row("SELECT task_id FROM tasks WHERE seq = 1", [], |r| r.get(0))
            .unwrap();
        finish::worktree_of(self.store.conn(), &task)
            .unwrap()
            .unwrap()
    }

    fn rev(&self, rev: &str) -> String {
        git(&self.root, &["rev-parse", rev])
    }

    fn log(&self, w: &str, file: &str) -> PathBuf {
        let task = &self.worktree().task_id.unwrap();
        self.tools.evidence_root.join(task).join(w).join(file)
    }
}

/// A started worker of `persona` that claims the next task it can.
fn worker(tx: &rusqlite::Transaction<'_>, run: &Run, persona: &str) -> anyhow::Result<String> {
    let n: i64 = tx.query_row("SELECT count(*) FROM workers", [], |r| r.get(0))?;
    let id = workers::insert(
        tx,
        &NewWorker {
            run_id: run.run_id.clone(),
            persona: persona.into(),
            role: Some(format!("{persona}-{}", n + 1)),
            ..Default::default()
        },
    )?;
    workers::supervisor_started(tx, &id, 4242, Some(1))?;
    workers::agent_started(tx, &id, 4243, Some(2))?;
    tasks::claim(tx, &id)?;
    Ok(id)
}

fn step<'a>(r: &'a FinishReport, name: &str) -> &'a Step {
    r.steps.iter().find(|s| s.name == name).unwrap()
}

fn note(r: &FinishReport, name: &str) -> String {
    step(r, name).note.clone().unwrap_or_default()
}

fn verdict(r: &FinishReport, name: &str) -> String {
    step(r, name).verdict.clone().unwrap_or_default()
}

// ---- land (#141) ----

#[test]
fn a_clean_branch_lands_on_the_run_branch_by_a_ref_update() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(step(&r, "land").status, "passed");
    assert_eq!(verdict(&r, "land"), head);
    assert_eq!(
        note(&r, "land"),
        format!("{BRANCH} landed on {RUN_BRANCH} at {}", &head[..12])
    );
    assert_eq!(w.rev(RUN_BRANCH), head);
    assert_eq!(git(&w.root, &["branch", "--show-current"]), "main");
    let wt = w.worktree();
    assert_eq!(wt.status, "landed");
    assert_eq!(wt.landed_commit.as_deref(), Some(head.as_str()));
    assert_eq!(
        std::fs::read_to_string(w.log(&id, "1-land.log")).unwrap(),
        format!("{}\n", note(&r, "land"))
    );
}

#[test]
fn a_checked_out_run_branch_is_fast_forwarded_by_a_merge() {
    let mut w = World::new(&[StepName::Land], false);
    git(&w.root, &["switch", "--quiet", RUN_BRANCH]);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(git(&w.root, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(w.root.join("feature.txt")).unwrap(),
        "work\n"
    );
}

#[test]
fn a_rebase_conflict_lands_nothing_and_aborts_the_rebase() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, path) = w.task(None, "feature.txt");
    git(&w.root, &["switch", "--quiet", RUN_BRANCH]);
    let moved = commit(&w.root, "feature.txt", "other\n");
    git(&w.root, &["switch", "--quiet", "main"]);
    let r = w.finish(&id);
    assert!(!r.complete);
    assert_eq!(r.stopped_at.as_deref(), Some("land"));
    assert_eq!(step(&r, "land").status, "failed");
    assert_eq!(verdict(&r, "land"), "not landed");
    let why = format!(
        "rebase on {RUN_BRANCH} conflicts; resolve it with git rebase {RUN_BRANCH} in {}, commit, then run finish again",
        path.display()
    );
    assert_eq!(note(&r, "land"), why);
    assert_eq!(r.message, format!("T-1 land: {why}"));
    assert_eq!(w.rev(RUN_BRANCH), moved, "the run branch is unchanged");
    let wt = w.worktree();
    assert_eq!(wt.status, "open");
    assert_eq!(wt.reason.as_deref(), Some(why.as_str()));
    assert_eq!(
        git(&path, &["status", "--porcelain"]),
        "",
        "no rebase is left"
    );
}

#[test]
fn a_red_check_after_the_rebase_lands_nothing() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, _) = w.task(Some("false"), "feature.txt");
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "land"), "not landed");
    assert_eq!(
        note(&r, "land"),
        format!(
            "after the rebase: `false` failed; read {}",
            w.log(&id, "1-land.log").display()
        )
    );
    assert_eq!(w.rev(RUN_BRANCH), w.base);
    assert_eq!(w.worktree().status, "open");
}

#[test]
fn a_run_branch_that_moves_during_1_check_is_rebased_and_checked_again() {
    let mut w = World::new(&[StepName::Land], false);
    let top = w.top();
    let count = top.join("count");
    let mark = top.join("moved");
    let check = format!(
        "echo x >> {c}; test -f {m} || {{ touch {m}; {mv}; }}",
        c = count.display(),
        m = mark.display(),
        mv = move_run_branch_script(&w.root)
    );
    let (id, path) = w.task(Some(&check), "feature.txt");
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(std::fs::read_to_string(&count).unwrap(), "x\nx\n");
    let head = git(&path, &["rev-parse", "HEAD"]);
    assert_eq!(w.rev(RUN_BRANCH), head);
    let moved = git(&path, &["rev-parse", "HEAD~1"]);
    assert_eq!(git(&w.root, &["log", "-1", "--format=%s", &moved]), "moved");
}

#[test]
fn a_run_branch_that_moves_during_every_check_lands_nothing() {
    let mut w = World::new(&[StepName::Land], false);
    let count = w.top().join("count");
    let check = format!(
        "echo x >> {c}; {mv}",
        c = count.display(),
        mv = move_run_branch_script(&w.root)
    );
    let (id, path) = w.task(Some(&check), "feature.txt");
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "land"), "not landed");
    assert_eq!(
        note(&r, "land"),
        format!("{RUN_BRANCH} moved during each of 5 checks; run finish again")
    );
    assert_eq!(std::fs::read_to_string(&count).unwrap().lines().count(), 5);
    assert_ne!(w.rev(RUN_BRANCH), git(&path, &["rev-parse", "HEAD"]));
    assert_eq!(w.worktree().status, "open");
}

#[test]
fn a_land_pushes_and_a_failed_push_is_retried_by_the_next_finish() {
    let mut w = World::new(&[StepName::Land], true);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    let remote = w.top().join("remote.git");
    git(
        &w.root,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "land"), "landed, not pushed");
    assert!(
        note(&r, "land").starts_with(&format!("push of {RUN_BRANCH} to origin failed: ")),
        "{r:?}"
    );
    let wt = w.worktree();
    assert_eq!(wt.status, "landed");
    assert_eq!(wt.landed_commit.as_deref(), Some(head.as_str()));
    assert_eq!(w.rev(RUN_BRANCH), head, "the land stands");

    git(
        &w.root,
        &["remote", "set-url", "origin", &remote.to_string_lossy()],
    );
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(
        note(&r, "land"),
        format!("{RUN_BRANCH} pushed to origin; it holds {head}")
    );
    assert_eq!(git(&remote, &["rev-parse", RUN_BRANCH]), head);
}

#[test]
fn a_land_with_a_remote_names_the_push() {
    let mut w = World::new(&[StepName::Land], true);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    let r = w.finish(&id);
    assert_eq!(
        note(&r, "land"),
        format!(
            "{BRANCH} landed on {RUN_BRANCH} at {}; pushed to origin",
            &head[..12]
        )
    );
    let first = note(&r, "land");
    let r = w.finish(&id);
    assert!(r.complete);
    assert_eq!(step(&r, "land").rounds, 1, "a passed land stands");
    assert_eq!(note(&r, "land"), first);
}

#[test]
fn a_task_that_is_not_landing_lands_nothing() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, path) = w.task(None, "feature.txt");
    let worker = workers::get(w.store.conn(), &id).unwrap();
    let task = tasks::get(w.store.conn(), worker.task_id.as_deref().unwrap()).unwrap();
    assert_eq!(task.status, "running");
    let evidence = w.top().join("ev");
    std::fs::create_dir_all(&evidence).unwrap();
    let run = w.run.clone();
    let tools = w.tools.clone();
    let cx = Ctx {
        tools: &tools,
        run: &run,
        task: &task,
        worker: &worker,
        worktree: Some(w.worktree()),
        dir: path,
        root: w.root.clone(),
        evidence_dir: evidence,
        seq: 1,
    };
    let o = land::land(&mut w.store, &cx).unwrap();
    assert_eq!(o.verdict.as_deref(), Some("not landed"));
    assert_eq!(o.evidence, None);
    assert_eq!(
        o.note,
        "T-1 is running: nothing landed. Stop work, then run horch done"
    );
    assert_eq!(w.worktree().status, "open");
    assert_eq!(w.rev(RUN_BRANCH), w.base);
}

// ---- unsafe_to_remove, remove, retry_cleanup (#141) ----

#[test]
fn unsafe_to_remove_names_the_first_reason_work_could_be_lost() {
    let mut w = World::new(&[StepName::Land], true);
    let (_, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    let g = w.tools.git.clone();
    let why = |wt: &WorktreeRow| {
        land::unsafe_to_remove(&g, &w.root, wt, RUN_BRANCH, Some("origin")).unwrap()
    };
    let wt = w.worktree();
    let gone = WorktreeRow {
        path: w.top().join("gone").to_string_lossy().into_owned(),
        ..wt.clone()
    };
    assert_eq!(why(&gone), None, "a missing folder loses nothing");
    // Dirty wins over the unlanded commit.
    std::fs::write(path.join("scratch.txt"), "x\n").unwrap();
    assert_eq!(
        why(&wt).as_deref(),
        Some("1 uncommitted or untracked files, e.g. ?? scratch.txt")
    );
    git(&path, &["stash", "push", "--quiet", "--include-untracked"]);
    let stash = git(&w.root, &["stash", "list"]);
    assert_eq!(why(&wt), Some(format!("a stash names {BRANCH}: {stash}")));
    git(&path, &["stash", "drop", "--quiet"]);
    assert_eq!(
        why(&wt),
        Some(format!("1 commits on {BRANCH} are not on {RUN_BRANCH}"))
    );
    git(
        &w.root,
        &["update-ref", &format!("refs/heads/{RUN_BRANCH}"), &head],
    );
    let landed = WorktreeRow {
        landed_commit: Some(head.clone()),
        ..wt.clone()
    };
    assert_eq!(why(&wt), None, "no landed commit: the remote is not asked");
    assert_eq!(
        why(&landed),
        Some(format!("origin/{RUN_BRANCH} does not have {head} yet"))
    );
    git(&w.root, &["push", "--quiet", "origin", RUN_BRANCH]);
    assert_eq!(why(&landed), None);
}

#[test]
fn a_dirty_worktree_is_kept_by_cleanup() {
    let mut w = World::new(&[StepName::Land, StepName::Cleanup], false);
    let (id, path) = w.task(None, "feature.txt");
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert!(!path.exists());
    let mut w = World::new(&[StepName::Land], false);
    let (id, path) = w.task(None, "feature.txt");
    assert!(w.finish(&id).complete);
    std::fs::write(path.join("scratch.txt"), "x\n").unwrap();
    let o = land::remove(&w.store, &w.tools.git, &w.worktree(), RUN_BRANCH, None).unwrap();
    assert_eq!(o.verdict.as_deref(), Some("kept"));
    assert_eq!(
        o.note,
        "the worktree stays: 1 uncommitted or untracked files, e.g. ?? scratch.txt"
    );
    assert!(path.exists());
    assert_eq!(w.worktree().status, "landed");
}

#[test]
fn a_failed_branch_delete_is_retried_by_the_watchdog() {
    let mut w = World::new(&[StepName::Land, StepName::Cleanup], false);
    let (id, path) = w.task(None, "feature.txt");
    let lock = w
        .root
        .join(".git/refs/heads")
        .join(format!("{BRANCH}.lock"));
    std::fs::write(&lock, "").unwrap();
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "cleanup"), "cleanup failed");
    let n = note(&r, "cleanup");
    assert!(n.starts_with("git branch -d: "), "{n}");
    assert!(n.ends_with("; the watchdog retries it"), "{n}");
    let wt = w.worktree();
    assert_eq!(wt.status, "cleanup_failed");
    assert_eq!(
        wt.reason.as_deref(),
        n.strip_suffix("; the watchdog retries it")
    );
    assert!(!path.exists(), "the folder went before the branch");

    // A cleanup_failed row with no task is skipped.
    w.store
        .conn()
        .execute(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, origin, created_at)
             VALUES ('orphan', ?1, '/nonexistent/orphan', 'x', 'main', 'cleanup_failed', 'native', '2026-01-01T00:00:00Z')",
            [w.root.to_string_lossy()],
        )
        .unwrap();
    std::fs::remove_file(&lock).unwrap();
    let out = land::retry_cleanup(&mut w.store, &w.tools.git).unwrap();
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].0, path.to_string_lossy());
    assert_eq!(out[0].1.verdict.as_deref(), Some("0 bytes freed"));
    assert_eq!(out[0].1.note, format!("{} is gone", path.display()));
    assert_eq!(w.worktree_by_path(&path), "cleaned");
    assert!(!git(&w.root, &["branch", "--list", BRANCH]).contains(BRANCH));
}

impl World {
    fn worktree_by_path(&self, path: &Path) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT status FROM worktrees WHERE path = ?1",
                [path.to_string_lossy()],
                |r| r.get(0),
            )
            .unwrap()
    }
}

// ---- complexity-sweep (#138) ----

/// An analyser script, run as `node <script>` with node = /bin/sh, so no
/// new executable is started (macOS scans each one at its first exec).
fn analyser(w: &mut World, body: &str) -> PathBuf {
    let cli = w.top().join("cli.js");
    std::fs::write(&cli, body).unwrap();
    w.tools.complexity_cli = Some(cli.clone());
    cli
}

#[test]
fn complexity_sweep_skips_files_the_analyser_cannot_read() {
    let mut w = World::new(&[StepName::ComplexitySweep], false);
    let (id, _) = w.task(None, "feature.rs");
    let r = w.finish(&id);
    assert!(r.complete);
    assert_eq!(step(&r, "complexity-sweep").status, "not_applicable");
    assert_eq!(
        note(&r, "complexity-sweep"),
        "no changed file is in a language the analyser reads"
    );
    assert!(step(&r, "complexity-sweep").judged_rev.is_some());
}

#[test]
fn complexity_sweep_without_an_analyser_fails() {
    let mut w = World::new(&[StepName::ComplexitySweep], false);
    let (id, _) = w.task(None, "lib.ts");
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "complexity-sweep"), "no analyser");
    assert_eq!(
        note(&r, "complexity-sweep"),
        "the complexity-sweep skill's analyser was not found. Install the code plugin's complexity-sweep skill (and run npm install in its code/core and code/cli), or set HORCH_COMPLEXITY_CLI to its code/cli/dist/index.js"
    );
    assert_eq!(step(&r, "complexity-sweep").judged_rev, None);
}

#[test]
fn complexity_sweep_fails_when_the_analyser_prints_nothing() {
    let mut w = World::new(&[StepName::ComplexitySweep], false);
    analyser(&mut w, "echo boom >&2\n");
    let (id, _) = w.task(None, "lib.ts");
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "complexity-sweep"), "no output");
    assert_eq!(
        note(&r, "complexity-sweep"),
        "the analyser printed nothing: boom"
    );
    assert_eq!(
        step(&r, "complexity-sweep").evidence_path.as_deref(),
        Some(w.log(&id, "1-complexity.json").to_string_lossy().as_ref())
    );
    assert_eq!(step(&r, "complexity-sweep").judged_rev, None);
}

#[test]
fn complexity_sweep_passes_with_no_violation_and_names_the_files() {
    let mut w = World::new(&[StepName::ComplexitySweep], false);
    let args = w.top().join("args");
    analyser(
        &mut w,
        &format!(
            "printf '%s\\n' \"$@\" > {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            args.display()
        ),
    );
    let (id, path) = w.task(None, "lib.ts");
    commit(&path, "notes.txt", "x\n");
    commit(&path, "b.py", "x = 1\n");
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(verdict(&r, "complexity-sweep"), "0 violations");
    assert_eq!(note(&r, "complexity-sweep"), "2 files analysed");
    assert_eq!(
        std::fs::read_to_string(args).unwrap(),
        "--format\njson\n--threshold-action\nnone\nb.py\nlib.ts\n"
    );
    assert!(step(&r, "complexity-sweep").judged_rev.is_some());
}

#[test]
fn complexity_sweep_fails_on_a_violation_in_a_changed_function() {
    let mut w = World::new(&[StepName::ComplexitySweep], false);
    analyser(
        &mut w,
        r#"echo '{"files":[{"filePath":"lib.ts","functions":[{"line":1}]}],"violations":[{"functionName":"f","filePath":"lib.ts","line":1,"metric":"cyclomaticComplexity","value":12,"threshold":10}]}'
"#,
    );
    let (id, _) = w.task(None, "lib.ts");
    let r = w.finish(&id);
    let evidence = w.log(&id, "1-complexity.json");
    assert_eq!(verdict(&r, "complexity-sweep"), "1 violations");
    assert_eq!(
        note(&r, "complexity-sweep"),
        format!(
            "reduce the complexity of the functions you changed, commit, then run finish again: lib.ts:1 f cyclomaticComplexity=12 (max 10). Full analysis: {}",
            evidence.display()
        )
    );
    assert!(step(&r, "complexity-sweep").judged_rev.is_some());
}

// ---- pr (#146) ----

const FAKE_GH: &str = r#"#!/bin/sh
d="$(dirname "$0")/ghstate"
n=$(ls "$d/calls" | wc -l | tr -d ' ')
printf '%s\0' "$@" > "$d/calls/$n"
case "$1 $2" in
"pr create") printf 'Creating pull request\nhttps://github.com/o/r/pull/7\n';;
"pr view") cat "$d/view.json";;
"pr edit") cat "$5" > "$d/edited";;
esac
"#;

/// A fake gh, warmed once, whose `pr view` prints `ghstate/view.json`.
fn fake_gh(w: &World) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let state = w.top().join("ghstate");
    std::fs::create_dir_all(state.join("calls")).unwrap();
    std::fs::write(&w.tools.gh, FAKE_GH).unwrap();
    std::fs::set_permissions(&w.tools.gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Command::new(&w.tools.gh)
        .arg("warm")
        .status()
        .unwrap()
        .success());
    std::fs::remove_dir_all(state.join("calls")).unwrap();
    std::fs::create_dir_all(state.join("calls")).unwrap();
    state
}

fn gh_calls(state: &Path) -> Vec<Vec<String>> {
    let mut names: Vec<u32> = std::fs::read_dir(state.join("calls"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().parse().unwrap())
        .collect();
    names.sort();
    names
        .iter()
        .map(|n| {
            std::fs::read_to_string(state.join("calls").join(n.to_string()))
                .unwrap()
                .split_terminator('\0')
                .map(String::from)
                .collect()
        })
        .collect()
}

fn view(state: &Path, body: &str, head: &str) {
    std::fs::write(
        state.join("view.json"),
        serde_json::json!({"body": body, "headRefOid": head}).to_string(),
    )
    .unwrap();
}

#[test]
fn pr_has_no_pull_request_without_a_remote() {
    let mut w = World::new(&[StepName::Pr], false);
    let (id, _) = w.task(None, "feature.txt");
    let r = w.finish(&id);
    assert_eq!(step(&r, "pr").status, "not_applicable");
    assert_eq!(
        note(&r, "pr"),
        "the run has no remote, so it has no pull request"
    );
}

#[test]
fn pr_creates_the_pull_request_and_writes_the_section_of_the_task() {
    let mut w = World::new(&[StepName::Land, StepName::Pr], true);
    let state = fake_gh(&w);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    view(&state, "", &head);
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(verdict(&r, "pr"), "PR #7");
    assert_eq!(note(&r, "pr"), "PR #7 holds T-1");
    let section = format!(
        "<!-- horch:task T-1 -->\n### T-1 feature\n\nLanded by opus-1.\n\n- Commit: `{}`\n- code-sweep: -\n- complexity-sweep: -\n- check: -\n<!-- /horch:task T-1 -->",
        &head[..12]
    );
    let created = format!(
        "Run `r1`: tasks land on `{RUN_BRANCH}`; this PR brings them to `main`.\n\n{section}"
    );
    let calls = gh_calls(&state);
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_eq!(
        calls[0],
        [
            "pr",
            "create",
            "--base",
            "main",
            "--head",
            RUN_BRANCH,
            "--title",
            "r1: fleet run",
            "--body",
            &created
        ]
    );
    assert_eq!(calls[1], ["pr", "view", "7", "--json", "body,headRefOid"]);
    let file = w.log(&id, "2-pr-body.md");
    assert_eq!(
        calls[2],
        ["pr", "edit", "7", "--body-file", &file.to_string_lossy()]
    );
    assert_eq!(
        std::fs::read_to_string(state.join("edited")).unwrap(),
        section
    );
    let run = runs::get(w.store.conn(), &w.run.run_id).unwrap();
    assert_eq!(run.pr_number, Some(7));
    assert_eq!(run.pr_url.as_deref(), Some("https://github.com/o/r/pull/7"));

    // The next pr run finds the PR, and an unchanged body is not edited.
    view(&state, &section, &head);
    w.store
        .conn()
        .execute(
            "UPDATE task_steps SET status = 'pending' WHERE name = 'pr'",
            [],
        )
        .unwrap();
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    let calls = gh_calls(&state);
    assert_eq!(calls.len(), 4, "1 view, no create, no edit: {calls:?}");
    assert_eq!(calls[3][1], "view");
}

#[test]
fn pr_fails_when_its_head_lacks_the_landed_commit() {
    let mut w = World::new(&[StepName::Land, StepName::Pr], true);
    let state = fake_gh(&w);
    let (id, path) = w.task(None, "feature.txt");
    let head = git(&path, &["rev-parse", "HEAD"]);
    view(&state, "", &w.base.clone());
    let r = w.finish(&id);
    assert_eq!(r.stopped_at.as_deref(), Some("pr"));
    assert_eq!(verdict(&r, "pr"), "PR head behind", "{r:?}");
    assert_eq!(
        note(&r, "pr"),
        format!(
            "PR #7 head {} does not contain {head}: the run branch is not pushed; run horch ledger finish to retry the push",
            w.base
        )
    );
}

// ---- code-sweep (#137) ----

const REVIEWER_NAMES: &[&str] = &[
    "bug-hunter",
    "vulnerability-scanner",
    "structure-inspector",
    "efficiency-auditor",
    "clarity-checker",
    "coverage-analyst",
    "contract-guardian",
    "waste-detector",
    "reliability-auditor",
    "dependency-examiner",
    "power-of-ten-reviewer",
    "interface-auditor",
];

fn sweep_world() -> World {
    let mut w = World::new(&[StepName::CodeSweep], false);
    let sweep = w.top().join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for r in REVIEWER_NAMES {
        std::fs::write(sweep.join("reviewers").join(format!("{r}.md")), "x\n").unwrap();
    }
    w.tools.code_sweep_dir = Some(sweep);
    w
}

/// The review tasks of round 1: (short, findings file).
fn reviews(w: &World) -> Vec<(String, String)> {
    let mut stmt = w
        .store
        .conn()
        .prepare("SELECT seq, spec_ref FROM tasks WHERE kind = 'review' ORDER BY seq")
        .unwrap();
    stmt.query_map([], |r| {
        Ok((format!("T-{}", r.get::<_, i64>(0)?), r.get(1)?))
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

/// Every review is in, with these findings files (empty: none written).
fn reviews_in(w: &World, files: &[&str]) {
    for ((_, file), body) in reviews(w).iter().zip(files) {
        std::fs::write(file, body).unwrap();
    }
    w.store
        .conn()
        .execute("UPDATE tasks SET status = 'done' WHERE kind = 'review'", [])
        .unwrap();
}

const NONE: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";

#[test]
fn code_sweep_without_a_change_or_a_skill() {
    let mut w = sweep_world();
    let id = w.claimed("opus", |_| {});
    finish::worktree_new(&mut w.store, &w.tools.git, &id, "feat").unwrap();
    let r = w.finish(&id);
    assert_eq!(step(&r, "code-sweep").status, "not_applicable");
    assert_eq!(note(&r, "code-sweep"), "the branch changes no file");

    let mut w = World::new(&[StepName::CodeSweep], false);
    let (id, _) = w.task(None, "feature.txt");
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "code-sweep"), "no skill");
    assert_eq!(
        note(&r, "code-sweep"),
        "the code-sweep skill was not found. Install the dev plugin's code-sweep skill, or set HORCH_CODE_SWEEP_DIR to its folder (the one that holds reviewers/)"
    );
}

#[test]
fn code_sweep_queues_a_round_then_waits_for_it() {
    let mut w = sweep_world();
    let (id, _) = w.task(None, "feature.txt");
    let r = w.finish(&id);
    assert_eq!(r.stopped_at.as_deref(), Some("code-sweep"));
    assert_eq!(step(&r, "code-sweep").status, "running");
    assert_eq!(verdict(&r, "code-sweep"), "waiting");
    assert_eq!(
        note(&r, "code-sweep"),
        "round 1: 4 reviewer tasks queued (T-2, T-3, T-4, T-5) for codex-reviewer. horch wakes the orchestrator to spawn them, and tells you when the reviews are in. End your turn now"
    );
    assert_eq!(r.queued_reviews.len(), 4);
    let files: Vec<String> = reviews(&w).into_iter().map(|(_, f)| f).collect();
    assert!(
        files[0].ends_with("round-1/bug-hunter-findings.md"),
        "{files:?}"
    );
    assert!(files[1].ends_with("round-1/clarity-checker-findings.md"));
    assert!(files[2].ends_with("round-1/structure-inspector-findings.md"));
    assert!(files[3].ends_with("round-1/coverage-analyst-findings.md"));
    let r = w.finish(&id);
    assert_eq!(
        note(&r, "code-sweep"),
        "round 1: waiting for reviewer tasks T-2, T-3, T-4, T-5. End your turn; horch tells you when they are in"
    );
}

#[test]
fn code_sweep_passes_and_writes_the_merged_report() {
    let mut w = sweep_world();
    let (id, _) = w.task(None, "feature.txt");
    w.finish(&id);
    reviews_in(
        &w,
        &[
            NONE,
            "## Critical\n- none\n## Important\n- none\n## Minor\n- a.rs:1 name it\n",
            NONE,
            NONE,
        ],
    );
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(verdict(&r, "code-sweep"), "PASS");
    assert_eq!(note(&r, "code-sweep"), "round 1: PASS");
    let evidence = w.log(&id, "1-code-sweep-report.md");
    assert_eq!(
        std::fs::read_to_string(&evidence).unwrap(),
        "# Code sweep of T-1 (round 1)\n\nOverall verdict: PASS\n\n\
         | Reviewer | Verdict | Critical | Important | Minor |\n|---|---|---|---|---|\n\
         | bug-hunter | PASS | 0 | 0 | 0 |\n\
         | clarity-checker | PASS | 0 | 0 | 1 |\n\
         | structure-inspector | PASS | 0 | 0 | 0 |\n\
         | coverage-analyst | PASS | 0 | 0 | 0 |\n\
         \n## Critical\n\n## Important\n\n## Minor\n- F1 [clarity-checker] a.rs:1 name it\n"
    );
}

#[test]
fn code_sweep_fails_on_a_missing_findings_file() {
    let mut w = sweep_world();
    let (id, _) = w.task(None, "feature.txt");
    w.finish(&id);
    reviews_in(&w, &[NONE, "", NONE, "## Critical\n- a.rs:1 boom\n"]);
    let r = w.finish(&id);
    let evidence = w.log(&id, "1-code-sweep-report.md");
    assert_eq!(verdict(&r, "code-sweep"), "FAIL");
    assert_eq!(
        note(&r, "code-sweep"),
        format!(
            "a reviewer finished without a findings file; read {}. To review this tree again, run horch ledger finish --rereview",
            evidence.display()
        )
    );
    let report = std::fs::read_to_string(&evidence).unwrap();
    assert!(report.contains("Overall verdict: FAIL\n"), "{report}");
    assert!(
        report.contains("| clarity-checker | MISSING | 0 | 0 | 0 |"),
        "{report}"
    );
    assert!(
        report.contains("| coverage-analyst | FAIL | 1 | 0 | 0 |"),
        "{report}"
    );
}

#[test]
fn code_sweep_fails_on_a_critical_finding_until_it_is_waived() {
    let mut w = sweep_world();
    let (id, _) = w.task(None, "feature.txt");
    w.finish(&id);
    reviews_in(
        &w,
        &[
            "## Critical\n- a.rs:1 boom\n## Important\n- a.rs:2 meh\n",
            NONE,
            NONE,
            NONE,
        ],
    );
    let r = w.finish(&id);
    let evidence = w.log(&id, "1-code-sweep-report.md");
    assert_eq!(verdict(&r, "code-sweep"), "FAIL");
    assert_eq!(
        note(&r, "code-sweep"),
        format!(
            "1 Critical findings (F1). Fix them, commit, and run finish again: the change is reviewed again. Report: {}",
            evidence.display()
        )
    );
    let task = tasks::get(w.store.conn(), &w.worktree().task_id.unwrap()).unwrap();
    codesweep::waive(&w.store, &w.tools.evidence_root, &task, "F1", "a test").unwrap();
    let r = w.finish(&id);
    assert_eq!(verdict(&r, "code-sweep"), "WARN");
    let answers = Path::new(&reviews(&w)[0].1).with_file_name("answers.md");
    assert_eq!(
        note(&r, "code-sweep"),
        format!(
            "1 Important findings without an answer (F2). Fix each one and commit (the change is reviewed again), or answer it in {} as `F<n>: <reason>`. Then run finish again. Report: {}",
            answers.display(),
            evidence.display()
        )
    );
    std::fs::write(&answers, "F2: the caller checks it\n").unwrap();
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(verdict(&r, "code-sweep"), "WARN");
    assert_eq!(note(&r, "code-sweep"), "round 1: WARN");
    let report = std::fs::read_to_string(&evidence).unwrap();
    assert!(report.contains("Overall verdict: WARN\n"), "{report}");
    assert!(
        report.contains("| bug-hunter | FAIL | 1 | 1 | 0 |"),
        "{report}"
    );
    assert!(
        report.contains(
            "\n## Critical\n- F1 [bug-hunter] a.rs:1 boom — waived by the orchestrator: a test\n"
        ),
        "{report}"
    );
    assert!(
        report.contains(
            "\n## Important\n- F2 [bug-hunter] a.rs:2 meh — answered: the caller checks it\n"
        ),
        "{report}"
    );
}

// ---- finish and done (#144) ----

#[test]
fn finish_refuses_a_review_task_and_a_task_without_its_worktree() {
    let mut w = sweep_world();
    let id = w.claimed("opus", |_| {});
    let e = finish::finish(&mut w.store, &w.tools, &id).unwrap_err();
    assert_eq!(
        e.to_string(),
        "T-1 has no worktree. Make it with horch ledger worktree new <name>, work and commit there, then run finish"
    );
    finish::worktree_new(&mut w.store, &w.tools.git, &id, "feat").unwrap();
    commit(&PathBuf::from(w.worktree().path), "feature.txt", "x\n");
    w.finish(&id);
    let run = w.run.clone();
    let reviewer = w
        .store
        .immediate(|tx| worker(tx, &run, "codex-reviewer"))
        .unwrap();
    let e = finish::finish(&mut w.store, &w.tools, &reviewer).unwrap_err();
    assert_eq!(
        e.to_string(),
        "T-2 is a review task: it has no finish steps; run horch ledger done"
    );
}

#[test]
fn a_passed_step_stands_until_the_tree_changes_and_after_the_land() {
    let mut w = World::new(&[StepName::Check, StepName::Land], false);
    let count = w.top().join("count");
    let check = format!("echo x >> {}", count.display());
    let (id, path) = w.task(Some(&check), "feature.txt");
    let runs = || std::fs::read_to_string(&count).unwrap().lines().count();
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(
        r.message,
        "T-1: every finish step passed. Run: horch ledger done \"<summary>\""
    );
    assert_eq!(r.stopped_at, None);
    assert_eq!(runs(), 2, "check, then the check of the land");
    let r = w.finish(&id);
    assert!(r.complete);
    assert_eq!(runs(), 2, "nothing runs again on the same tree");
    // Once landed, a tree change with no new commit keeps the check.
    git(&path, &["reset", "--quiet", "--hard", &w.base]);
    let r = w.finish(&id);
    assert!(r.complete);
    assert_eq!(runs(), 2);
    // A commit after the land opens the steps from land on again (#154).
    git(&path, &["reset", "--quiet", "--hard", RUN_BRANCH]);
    let new = commit(&path, "more.txt", "y\n");
    let r = w.finish(&id);
    assert!(r.complete, "{r:?}");
    assert_eq!(runs(), 4, "check and land run again");
    assert_eq!(w.rev(RUN_BRANCH), new);
}

#[test]
fn a_changed_tree_runs_a_passed_step_again_before_the_land() {
    let mut w = World::new(&[StepName::Check], false);
    let count = w.top().join("count");
    let (id, path) = w.task(
        Some(&format!("echo x >> {}", count.display())),
        "feature.txt",
    );
    w.finish(&id);
    w.finish(&id);
    assert_eq!(std::fs::read_to_string(&count).unwrap(), "x\n");
    commit(&path, "more.txt", "y\n");
    let r = w.finish(&id);
    assert!(r.complete);
    assert_eq!(std::fs::read_to_string(&count).unwrap(), "x\nx\n");
    assert_eq!(step(&r, "check").rounds, 2);
}

#[test]
fn a_failed_step_stops_finish_and_names_the_step() {
    let mut w = World::new(&[StepName::Check, StepName::Land], false);
    let (id, _) = w.task(Some("false"), "feature.txt");
    let r = w.finish(&id);
    assert!(!r.complete);
    assert_eq!(r.stopped_at.as_deref(), Some("check"));
    assert_eq!(
        r.message,
        format!(
            "T-1 check: `false` failed; read {}; fix it",
            w.log(&id, "1-check.log").display()
        )
    );
    assert_eq!(step(&r, "land").status, "pending");
    let task = tasks::get(w.store.conn(), &w.worktree().task_id.unwrap()).unwrap();
    assert_eq!(task.status, "landing");
}

#[test]
fn done_checks_the_summary_and_the_steps() {
    let mut w = World::new(&[StepName::Check, StepName::Land], false);
    let (id, _) = w.task(Some("false"), "feature.txt");
    let e = finish::complete(&mut w.store, &id, " ", None).unwrap_err();
    assert_eq!(
        e.to_string(),
        "give a summary: horch ledger done \"<what you did, files, decisions, gotchas>\""
    );
    let not_finished = |open: &str| {
        format!(
            "T-1 is not finished: {open}. Run horch ledger finish until every step passes. \
             Only if the work truly cannot land: horch ledger done --unlanded \"<reason>\" \"<summary>\""
        )
    };
    let e = finish::complete(&mut w.store, &id, "s", None).unwrap_err();
    assert_eq!(e.to_string(), not_finished("no finish step ran yet"));
    w.finish(&id);
    let e = finish::complete(&mut w.store, &id, "s", None).unwrap_err();
    assert_eq!(
        e.to_string(),
        not_finished("check (failed), land (pending)")
    );
}

#[test]
fn done_after_every_step_passed_and_its_retry() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, _) = w.task(None, "feature.txt");
    w.finish(&id);
    let c = finish::complete(&mut w.store, &id, "all done", None).unwrap();
    assert_eq!(c.task, "T-1");
    assert!(!c.review && !c.unlanded && !c.already);
    assert_eq!(c.reviews_in_for, None);
    let task = tasks::get(w.store.conn(), &c.task_id).unwrap();
    assert_eq!(task.status, "done");
    assert_eq!(task.summary.as_deref(), Some("all done"));
    let wk = workers::get(w.store.conn(), &id).unwrap();
    assert_eq!(wk.status, "done");
    let again = finish::complete(&mut w.store, &id, "all done", None).unwrap();
    assert!(again.already);
    assert_eq!(again.task_id, c.task_id);
}

#[test]
fn done_unlanded_parks_the_task_and_its_worktree() {
    let mut w = World::new(&[StepName::Land], false);
    let (id, _) = w.task(Some("false"), "feature.txt");
    let c = finish::complete(&mut w.store, &id, "tried", Some("no way")).unwrap();
    assert!(c.unlanded);
    let task = tasks::get(w.store.conn(), &c.task_id).unwrap();
    assert_eq!(task.status, "cancelled");
    assert_eq!(task.summary.as_deref(), Some("UNLANDED: no way | tried"));
    let wt: (String, Option<String>) = w
        .store
        .conn()
        .query_row(
            "SELECT status, reason FROM worktrees WHERE task_id = ?1",
            [&c.task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(wt, ("unlanded".to_string(), Some("no way".to_string())));
}

#[test]
fn the_last_review_in_names_the_author_to_wake() {
    let mut w = sweep_world();
    let (id, _) = w.task(None, "feature.txt");
    w.finish(&id);
    let run = w.run.clone();
    let mut reviewers = Vec::new();
    for _ in 0..4 {
        reviewers.push(
            w.store
                .immediate(|tx| worker(tx, &run, "codex-reviewer"))
                .unwrap(),
        );
    }
    let files = reviews(&w);
    let e = finish::complete(&mut w.store, &reviewers[0], "0", None).unwrap_err();
    assert_eq!(
        e.to_string(),
        format!("write your findings file first: {}", files[0].1)
    );
    let mut woke = Vec::new();
    for (r, (_, file)) in reviewers.iter().zip(&files) {
        std::fs::write(file, NONE).unwrap();
        let c = finish::complete(&mut w.store, r, "0 findings", None).unwrap();
        assert!(c.review);
        woke.push(c.reviews_in_for);
    }
    assert_eq!(
        woke,
        vec![
            None,
            None,
            None,
            Some(("T-1".to_string(), Some("opus-1".to_string())))
        ]
    );
}

#[test]
fn worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree() {
    let mut w = World::new(&[StepName::Land], false);
    let shared = w.claimed("opus", |t| t.worktree_mode = WorktreeMode::Shared);
    let e = finish::worktree_new(&mut w.store, &w.tools.git, &shared, "x").unwrap_err();
    assert_eq!(
        e.to_string(),
        "T-1 works in the shared checkout; it has no worktree"
    );
    let id = w.claimed("opus", |_| {});
    std::fs::create_dir_all(w.root.join(".worktrees/T2-taken")).unwrap();
    let e = finish::worktree_new(&mut w.store, &w.tools.git, &id, "taken").unwrap_err();
    assert_eq!(
        e.to_string(),
        format!(
            "{} exists but the ledger has no row for it; pick another name",
            w.root.join(".worktrees/T2-taken").display()
        )
    );
    let first = finish::worktree_new(&mut w.store, &w.tools.git, &id, "Feat B!").unwrap();
    assert_eq!(first.branch, "fleet/r1-T2-feat-b");
    assert_eq!(first.base, RUN_BRANCH);
    assert_eq!(first.base_commit.as_deref(), Some(w.base.as_str()));
    assert_eq!(first.status, "open");
    assert!(Path::new(&first.path).ends_with(".worktrees/T2-feat-b"));
    // A replacement worker of the task gets the same worktree.
    let run = w.run.clone();
    w.store
        .immediate(|tx| {
            tx.execute(
                "UPDATE tasks SET status = 'queued', current_worker_id = NULL WHERE seq = 2",
                [],
            )?;
            tx.execute(
                "UPDATE workers SET status = 'stopped' WHERE worker_id = ?1",
                [&id],
            )?;
            Ok(())
        })
        .unwrap();
    let next = w.store.immediate(|tx| worker(tx, &run, "opus")).unwrap();
    let again = finish::worktree_new(&mut w.store, &w.tools.git, &next, "other").unwrap();
    assert_eq!(again.worktree_id, first.worktree_id);
    assert_eq!(again.worker_id.as_deref(), Some(next.as_str()));
}

#[test]
fn a_step_row_reads_back_every_column() {
    let mut w = World::new(&[StepName::Check], false);
    let (id, path) = w.task(Some("true"), "feature.txt");
    w.finish(&id);
    let task = w.worktree().task_id.unwrap();
    let steps = finish::steps_of(w.store.conn(), &task, &id).unwrap();
    assert_eq!(steps.len(), 1);
    let s = &steps[0];
    assert_eq!(
        (s.seq, s.name.as_str(), s.status.as_str(), s.rounds),
        (1, "check", "passed", 1)
    );
    assert_eq!(
        s.evidence_path.as_deref(),
        Some(w.log(&id, "1-check.log").to_string_lossy().as_ref())
    );
    assert_eq!(s.verdict.as_deref(), Some("exit 0"));
    assert_eq!(s.review_task_ids, None);
    assert_eq!(
        s.judged_rev.as_deref(),
        Some(git(&path, &["rev-parse", "HEAD^{tree}"]).as_str())
    );
    assert!(s.started_at.is_some() && s.ended_at.is_some());
    assert_eq!(s.note.as_deref(), Some("`true` passed"));
    let wt = w.worktree();
    assert_eq!(wt.task_id.as_deref(), Some(task.as_str()));
    assert_eq!(wt.worker_id.as_deref(), Some(id.as_str()));
    assert_eq!(wt.repo_root, w.root.to_string_lossy());
    assert_eq!(wt.path, path.to_string_lossy());
    assert_eq!(wt.branch, BRANCH);
    assert_eq!(wt.landed_commit, None);
    assert_eq!(wt.bytes_on_disk, None);
    assert_eq!(wt.reason, None);
}

// ---- `horch worktree drop`: what it refuses, and then removes nothing ------

fn dropped(
    w: &mut World,
    name: &str,
    reason: &str,
    keep_branch: bool,
) -> anyhow::Result<WorktreeRow> {
    let run_id = w.run.run_id.clone();
    worktree_ops::drop_worktree(
        &mut w.store,
        &w.tools.git,
        &run_id,
        name,
        reason,
        keep_branch,
    )
}

/// Each check of `drop_worktree` refuses alone, with its own message, and
/// leaves the folder and the branch where they were. Once the checks pass,
/// the folder goes, the row is abandoned with the reason, and the branch
/// stays because `--keep-branch` asked for it.
#[test]
fn a_drop_refuses_an_empty_reason_a_dirty_tree_and_a_stash_and_removes_nothing() {
    let mut w = World::new(&[StepName::Land], false);
    let (_id, path) = w.task(None, "feature.txt");

    let err = dropped(&mut w, "T1-feat", "  ", true).unwrap_err();
    assert_eq!(err.to_string(), "give a nonempty --reason");
    assert!(path.is_dir(), "an empty reason removed the folder");

    std::fs::write(path.join("scratch.txt"), "x\n").unwrap();
    let err = dropped(&mut w, "T1-feat", "cleanup", true).unwrap_err();
    assert_eq!(
        err.to_string(),
        "worktree has uncommitted or untracked changes; nothing removed"
    );
    assert!(path.is_dir(), "a dirty tree was removed");

    git(
        &path,
        &[
            "stash",
            "push",
            "--include-untracked",
            "--quiet",
            "-m",
            "parked",
        ],
    );
    let err = dropped(&mut w, "T1-feat", "cleanup", true).unwrap_err();
    assert_eq!(
        err.to_string(),
        "worktree branch has a stash; nothing removed"
    );
    assert!(path.is_dir(), "a stash was lost with its worktree");
    assert_eq!(w.worktree().status, "open");

    git(&path, &["stash", "drop", "--quiet"]);
    let row = dropped(&mut w, "T1-feat", "cleanup", true).unwrap();
    assert!(!path.exists());
    assert_eq!(
        (row.status.as_str(), row.reason.as_deref()),
        ("abandoned", Some("cleanup"))
    );
    git(&w.root, &["rev-parse", "--verify", BRANCH]);
}

/// A name that no registered worktree has is refused with the count, and a
/// worktree that sits on another branch than its recorded one is refused
/// even with `--keep-branch`, although it holds the same commit.
#[test]
fn a_drop_refuses_an_unknown_name_and_a_worktree_on_another_branch() {
    let mut w = World::new(&[StepName::Land], false);
    let (_id, path) = w.task(None, "feature.txt");

    let err = dropped(&mut w, "nope", "cleanup", true).unwrap_err();
    assert_eq!(
        err.to_string(),
        "expected one registered worktree named nope, found 0"
    );

    git(&path, &["checkout", "--quiet", "-b", "elsewhere"]);
    for keep_branch in [true, false] {
        let err = dropped(&mut w, "T1-feat", "cleanup", keep_branch).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "worktree is on elsewhere, not its recorded branch {BRANCH}; nothing removed (including with --keep-branch)"
            ),
            "keep_branch {keep_branch}"
        );
    }
    assert!(path.is_dir());
}
