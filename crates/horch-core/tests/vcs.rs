//! VCS and validation (phases B2 and B3): the git client, candidate
//! worktrees and the command validator, on temp repos with real git.
//!
//! Real git touches only temp dirs (NFR-07). Every git process gets an empty
//! `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM=1` and pinned identities and
//! dates, so hashes are the same on every machine. Without git on `PATH` the
//! git tests are skipped, unless `HORCH_REQUIRE_GIT=1`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use horch_core::evaluation::validator::{
    CommandValidator, GateSpec, GateStatus, ValidationReport, Validator,
};
use horch_core::ids::ExecutionId;
use horch_core::measure::digest::sha256_bytes;
use horch_core::vcs::git::{CheckoutLocation, CherryPick, GitCli, GitClient, GitIdentity};
use horch_core::vcs::worktree::{FrozenCandidate, WorktreeManager, WorktreeSpec};
use horch_marketplace::git::GitRunner;
use tempfile::TempDir;

/// The fixture's commit date, as in the e2e harness.
const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

fn freeze_at() -> DateTime<Utc> {
    "2026-10-02T10:00:00Z".parse().unwrap()
}

fn find_git() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
            "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
        );
    }
    found
}

/// A temp repo on `main` with one commit: `a.txt` (3 lines) and `b.txt`.
struct Fixture {
    _tmp: TempDir,
    root: PathBuf,
    repo: PathBuf,
    /// For fixture setup; pinned like `git`.
    runner: GitRunner,
    git: GitCli,
}

impl Fixture {
    fn new() -> Option<Self> {
        let bin = find_git()?;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let temp_root = std::env::temp_dir().canonicalize().unwrap();
        assert!(root.starts_with(&temp_root), "real git only on temp repos");
        let config = root.join("gitconfig");
        std::fs::write(&config, "").unwrap();
        let config = config.to_str().unwrap().to_owned();
        let pins = [
            ("GIT_CONFIG_GLOBAL", config.as_str()),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Horch Fixture"),
            ("GIT_AUTHOR_EMAIL", "fixture@horch.invalid"),
            ("GIT_COMMITTER_NAME", "Horch Fixture"),
            ("GIT_COMMITTER_EMAIL", "fixture@horch.invalid"),
            ("GIT_AUTHOR_DATE", GIT_DATE),
            ("GIT_COMMITTER_DATE", GIT_DATE),
        ];
        let mut runner = GitRunner::new(&bin);
        let mut git = GitCli::new(bin);
        for (k, v) in pins {
            runner = runner.with_env(k, v);
            git = git.with_env(k, v);
        }
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let f = Fixture {
            _tmp: tmp,
            root,
            repo,
            runner,
            git,
        };
        f.git_in(&f.repo, &["init", "-q", "-b", "main"]);
        std::fs::write(f.repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(f.repo.join("b.txt"), "bee\n").unwrap();
        f.commit_in(&f.repo, "base");
        Some(f)
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        self.runner.run(dir, args).unwrap().stdout_text()
    }

    fn commit_in(&self, dir: &Path, message: &str) -> String {
        self.git_in(dir, &["add", "-A"]);
        self.git_in(dir, &["commit", "-q", "-m", message]);
        self.git_in(dir, &["rev-parse", "HEAD"])
    }

    fn manager(&self) -> WorktreeManager<'_, GitCli> {
        WorktreeManager { git: &self.git }
    }

    fn spec(&self, label: &str, base: &str) -> WorktreeSpec {
        WorktreeSpec {
            repo: self.repo.clone(),
            root: self.root.join("wt"),
            exp8: "0a1b2c3d".into(),
            round_index: 1,
            label: label.into(),
            base_sha: base.into(),
        }
    }
}

fn execution() -> ExecutionId {
    ExecutionId::mint(freeze_at())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn cmp_04_n_worktrees_same_base_modify_same_file() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    let mgr = f.manager();
    let mut frozen = Vec::new();
    for label in ["c1", "c2", "c3"] {
        let spec = f.spec(label, &base);
        let path = mgr.create(&spec).unwrap();
        assert_eq!(path, f.root.join("wt").join(label));
        // Idempotent: a second create returns the same worktree.
        assert_eq!(mgr.create(&spec).unwrap(), path);
        std::fs::write(path.join("a.txt"), format!("one\ntwo by {label}\nthree\n")).unwrap();
        std::fs::write(path.join(format!("{label}.txt")), "new\n").unwrap();
        let c = mgr.freeze(&spec, &execution(), freeze_at()).unwrap();
        assert_eq!(c.branch, format!("mh/exp/0a1b2c3d/r1/{label}"));
        assert_eq!(c.base_sha, base);
        assert_eq!(c.frozen_at, "2026-10-02T10:00:00Z");
        assert_eq!(c.numstat.len(), 2, "{:?}", c.numstat);
        assert_eq!(c.numstat[0].path, "a.txt");
        assert_eq!(
            (c.numstat[0].added, c.numstat[0].deleted),
            (Some(1), Some(1))
        );
        // Freezing again with nothing new keeps the head.
        let again = mgr.freeze(&spec, &execution(), freeze_at()).unwrap();
        assert_eq!(again.head_sha, c.head_sha);
        assert_eq!(again.diff_digest, c.diff_digest);
        frozen.push(c);
    }
    let heads: BTreeSet<_> = frozen.iter().map(|c| c.head_sha.clone()).collect();
    assert_eq!(heads.len(), 3, "3 distinct head SHAs");
    for c in &frozen {
        let parent = f
            .git
            .rev_parse(&f.repo, &format!("{}^", c.head_sha))
            .unwrap();
        assert_eq!(parent.as_deref(), Some(base.as_str()));
        let author = f.git_in(
            &f.repo,
            &["log", "-1", "--format=%an <%ae> %aI %s", &c.head_sha],
        );
        assert_eq!(
            author,
            format!(
                "multi-herdr-dataset <dataset@multi-herdr.invalid> 2026-10-02T10:00:00+00:00 candidate {} frozen",
                c.label
            )
        );
    }
    // The main checkout is untouched.
    assert_eq!(f.git.head(&f.repo).unwrap(), base);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(read(&f.repo.join("a.txt")), "one\ntwo\nthree\n");
    assert_eq!(
        f.git.current_branch(&f.repo).unwrap().as_deref(),
        Some("main")
    );

    // A path held by a worktree on another branch is an error.
    let mut other_round = f.spec("c1", &base);
    other_round.round_index = 2;
    let err = mgr.create(&other_round).unwrap_err().to_string();
    assert!(err.contains("not on mh/exp/0a1b2c3d/r2/c1"), "{err}");

    // remove keeps the branch, and create checks it out again.
    let spec = f.spec("c2", &base);
    mgr.remove(&spec).unwrap();
    mgr.remove(&spec).unwrap();
    assert!(!spec.path().exists());
    let tip = f
        .git
        .rev_parse(&f.repo, &format!("refs/heads/{}", spec.branch()))
        .unwrap();
    assert_eq!(tip.as_deref(), Some(frozen[1].head_sha.as_str()));
    assert_eq!(
        f.git
            .branch_checkout_location(&f.repo, &spec.branch())
            .unwrap(),
        CheckoutLocation::NotCheckedOut
    );
    let path = mgr.create(&spec).unwrap();
    assert_eq!(f.git.head(&path).unwrap(), frozen[1].head_sha);
    assert_eq!(
        f.git
            .branch_checkout_location(&f.repo, &spec.branch())
            .unwrap(),
        CheckoutLocation::CheckedOut {
            path: path.clone(),
            clean: true
        }
    );

    // A plain directory in the way is never taken over.
    let blocked = f.spec("c9", &base);
    std::fs::create_dir_all(blocked.path()).unwrap();
    assert!(mgr.create(&blocked).is_err());
}

#[test]
fn cmp_08_freeze_deterministic_sha_and_numstat() {
    let mut results = Vec::new();
    for _ in 0..2 {
        let Some(f) = Fixture::new() else { return };
        let base = f.git.head(&f.repo).unwrap();
        let spec = f.spec("c1", &base);
        let mgr = f.manager();
        let path = mgr.create(&spec).unwrap();
        std::fs::write(path.join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        std::fs::remove_file(path.join("b.txt")).unwrap();
        std::fs::write(path.join("blob.bin"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
        let c = mgr.freeze(&spec, &execution(), freeze_at()).unwrap();
        let (patch, truncated) = f
            .git
            .diff_patch(&path, &base, &c.head_sha, usize::MAX)
            .unwrap();
        assert!(!truncated);
        assert_eq!(sha256_bytes(patch.as_bytes()), c.diff_digest);
        let (cut, truncated) = f.git.diff_patch(&path, &base, &c.head_sha, 10).unwrap();
        assert!(truncated);
        assert_eq!(cut, patch[..10]);
        results.push((base, c, f));
    }
    let (base_a, a, _fa) = &results[0];
    let (base_b, b, _fb) = &results[1];
    assert_eq!(base_a, base_b, "the fixture base is deterministic");
    assert_eq!(a.head_sha, b.head_sha);
    assert_eq!(a.numstat, b.numstat);
    assert_eq!(a.diff_digest, b.diff_digest);
    let by_path: Vec<_> = a
        .numstat
        .iter()
        .map(|l| (l.path.as_str(), l.added, l.deleted))
        .collect();
    assert_eq!(
        by_path,
        [
            ("a.txt", Some(2), Some(1)),
            ("b.txt", Some(0), Some(1)),
            ("blob.bin", None, None),
        ]
    );
}

#[test]
fn freeze_refuses_a_worktree_redirected_to_another_repo() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    let spec = f.spec("c1", &base);
    let path = f.manager().create(&spec).unwrap();
    // A candidate points its `.git` file at a repo it made, on a branch of
    // the same name.
    let other = f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    f.git_in(&other, &["init", "-q", "-b", &spec.branch()]);
    std::fs::write(other.join("x.txt"), "x\n").unwrap();
    f.commit_in(&other, "elsewhere");
    let gitdir = format!("gitdir: {}\n", other.join(".git").display());
    std::fs::write(path.join(".git"), gitdir).unwrap();
    std::fs::write(path.join("a.txt"), "changed\n").unwrap();

    let err = f
        .manager()
        .freeze(&spec, &execution(), freeze_at())
        .unwrap_err()
        .to_string();
    assert!(err.contains("worktree reports"), "{err}");
    let tip = f
        .git
        .rev_parse(&f.repo, &format!("refs/heads/{}", spec.branch()))
        .unwrap();
    assert_eq!(tip.as_deref(), Some(base.as_str()));
}

#[test]
fn git_cherry_pick_conflict_reports_paths() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    f.git_in(&f.repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(f.repo.join("a.txt"), "one\ntwo side\nthree\n").unwrap();
    f.commit_in(&f.repo, "side edits a");
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let side_c = f.commit_in(&f.repo, "side adds c");
    f.git_in(&f.repo, &["checkout", "-q", "main"]);
    std::fs::write(f.repo.join("a.txt"), "one\ntwo main\nthree\n").unwrap();
    let main = f.commit_in(&f.repo, "main edits a");

    let id = GitIdentity {
        name: "picker".into(),
        email: "picker@horch.invalid".into(),
        date: "2026-10-02T11:00:00Z".into(),
    };
    let r = f
        .git
        .cherry_pick(&f.repo, &format!("{base}..side"), &id)
        .unwrap();
    assert_eq!(
        r,
        CherryPick::Conflict {
            paths: vec!["a.txt".into()]
        }
    );
    // Aborted: the checkout is as it was.
    assert_eq!(f.git.head(&f.repo).unwrap(), main);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(f.git.rev_parse(&f.repo, "CHERRY_PICK_HEAD").unwrap(), None);

    // A clean pick reports the new head, committed as `id`.
    let CherryPick::Clean { head } = f.git.cherry_pick(&f.repo, &side_c, &id).unwrap() else {
        panic!("expected a clean pick");
    };
    assert_eq!(head, f.git.head(&f.repo).unwrap());
    assert_eq!(
        f.git_in(&f.repo, &["log", "-1", "--format=%cn %cI %an", &head]),
        "picker 2026-10-02T11:00:00+00:00 Horch Fixture"
    );
    assert_eq!(
        f.git.rev_list(&f.repo, &format!("{main}..{head}")).unwrap(),
        std::slice::from_ref(&head)
    );
    assert!(f.git.is_ancestor(&f.repo, &main, &head).unwrap());
    assert!(!f.git.is_ancestor(&f.repo, &head, &main).unwrap());
}

#[test]
fn git_update_ref_cas_mismatch_is_false() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    f.git_in(&f.repo, &["branch", "target"]);
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let next = f.commit_in(&f.repo, "next");
    let target = "refs/heads/target";

    // The ref holds `base`, not `next`: no swap, no error.
    assert!(!f.git.update_ref_cas(&f.repo, target, &next, &next).unwrap());
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(base.as_str())
    );
    assert!(f.git.update_ref_cas(&f.repo, target, &next, &base).unwrap());
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(next.as_str())
    );

    // Option-shaped arguments never reach git.
    assert!(f
        .git
        .update_ref_cas(&f.repo, "--stdin", &next, &base)
        .is_err());
    assert!(f.git.rev_parse(&f.repo, "--all").is_err());

    assert!(f.git.version().unwrap().starts_with("git version "));
    assert_eq!(f.git.toplevel(&f.repo).unwrap(), f.repo);
}

/// A frozen candidate over a plain temp dir: the validator needs no git.
fn candidate(worktree: &Path) -> FrozenCandidate {
    FrozenCandidate {
        label: "c1".into(),
        execution_id: execution(),
        worktree: worktree.to_path_buf(),
        branch: "mh/exp/0a1b2c3d/r1/c1".into(),
        base_sha: "a".repeat(40),
        head_sha: "b".repeat(40),
        numstat: Vec::new(),
        diff_digest: sha256_bytes(b""),
        frozen_at: "2026-10-02T10:00:00Z".into(),
    }
}

fn gate(name: &str, command: &str, timeout_ms: u64) -> GateSpec {
    GateSpec {
        name: name.into(),
        command: command.into(),
        timeout: Duration::from_millis(timeout_ms),
    }
}

/// Validate with fresh dirs; returns the report, the worktree and the
/// artifacts dir.
fn validate_in(
    tmp: &TempDir,
    setup: impl FnOnce(&Path),
    make: impl FnOnce(PathBuf) -> CommandValidator,
) -> (ValidationReport, PathBuf, PathBuf) {
    let worktree = tmp.path().join("wt");
    let artifacts = tmp.path().join("artifacts");
    std::fs::create_dir_all(&worktree).unwrap();
    setup(&worktree);
    let v = make(artifacts.clone());
    let report = v.validate(&candidate(&worktree)).unwrap();
    (report, worktree, artifacts)
}

#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[test]
fn cmp_09_gates_per_candidate_with_timeouts() {
    let tmp = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let (report, worktree, artifacts) = validate_in(
        &tmp,
        |_| {},
        |artifacts| {
            CommandValidator::new(
                vec![
                    gate("pass", "echo passing", 10_000),
                    gate("fail", "echo failing >&2; exit 3", 10_000),
                    gate("slow", "sleep 5 & echo $! > sleep.pid; wait", 300),
                ],
                artifacts,
                BTreeSet::new(),
            )
        },
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the timeout fired"
    );
    let statuses: Vec<_> = report.gates.iter().map(|g| g.status.clone()).collect();
    assert_eq!(
        statuses,
        [
            GateStatus::Passed,
            GateStatus::Failed { code: 3 },
            GateStatus::TimedOut
        ]
    );
    assert_eq!(report.label, "c1");
    assert_eq!(report.head_sha, "b".repeat(40));
    assert!((report.mechanical_score - 1.0 / 3.0).abs() < 1e-9);
    assert!(!report.eligible);
    assert_eq!(report.validation_id.len(), 36);

    for (g, (n, name)) in report
        .gates
        .iter()
        .zip([(1, "pass"), (2, "fail"), (3, "slow")])
    {
        let want = artifacts.join("c1").join(format!("gate-{n}-{name}.log"));
        assert_eq!(g.log_ref, want);
        let bytes = std::fs::read(&want).unwrap();
        assert_eq!(g.log_digest, sha256_bytes(&bytes));
        assert!(!g.log_truncated);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&want).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{}", want.display());
        }
    }
    assert_eq!(read(&report.gates[0].log_ref), "passing\n");
    assert_eq!(read(&report.gates[1].log_ref), "failing\n");
    let leftovers: Vec<_> = std::fs::read_dir(artifacts.join("c1"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| !n.ends_with(".log"))
        .collect();
    assert!(leftovers.is_empty(), "raw logs removed: {leftovers:?}");

    // The sleep the timed-out gate started is gone too.
    #[cfg(unix)]
    {
        let pid: i32 = read(&worktree.join("sleep.pid")).trim().parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while pid_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!pid_alive(pid), "sleep {pid} outlived its gate");
    }

    // No gates: score 0.0.
    let tmp = tempfile::tempdir().unwrap();
    let (report, _, _) = validate_in(
        &tmp,
        |_| {},
        |a| CommandValidator::new(Vec::new(), a, BTreeSet::new()),
    );
    assert!(report.gates.is_empty());
    assert_eq!(report.mechanical_score, 0.0);
}

#[test]
fn sec_07_gates_only_from_config() {
    let tmp = tempfile::tempdir().unwrap();
    let (report, worktree, _) = validate_in(
        &tmp,
        |wt| {
            // What a candidate or a judge might offer as a gate.
            std::fs::write(wt.join("gates.sh"), "touch candidate-marker\n").unwrap();
            std::fs::write(
                wt.join("judgment.json"),
                r#"{"gates":[{"name":"judge","command":"touch judge-marker"}],"run":"touch judge-marker"}"#,
            )
            .unwrap();
        },
        |a| {
            CommandValidator::new(
                vec![gate("configured", "touch configured-marker", 10_000)],
                a,
                BTreeSet::new(),
            )
        },
    );
    let names: Vec<_> = report.gates.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["configured"]);
    assert_eq!(report.gates[0].status, GateStatus::Passed);
    assert!(report.eligible);
    assert_eq!(report.mechanical_score, 1.0);
    assert!(worktree.join("configured-marker").exists());
    assert!(!worktree.join("candidate-marker").exists());
    assert!(!worktree.join("judge-marker").exists());
}

#[test]
fn validator_fault_fail_gate_does_not_run_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (report, worktree, _) = validate_in(
        &tmp,
        |_| {},
        |a| {
            CommandValidator::new(
                vec![
                    gate("build", "touch build-marker", 10_000),
                    gate("test", "true", 10_000),
                ],
                a,
                BTreeSet::from(["fail-gate:build".to_owned()]),
            )
        },
    );
    assert_eq!(
        report.gates[0].status,
        GateStatus::Error {
            reason: "fault".into()
        }
    );
    assert_eq!(report.gates[1].status, GateStatus::Passed);
    assert!(!worktree.join("build-marker").exists());
    assert!(report.gates[0].log_ref.exists());
    assert!(!report.eligible);
}

#[test]
fn validator_strips_api_key_and_redacts() {
    let tmp = tempfile::tempdir().unwrap();
    let (report, worktree, _) = validate_in(
        &tmp,
        |_| {},
        |a| {
            CommandValidator::new(
                vec![gate(
                    "leak",
                    "echo \"key:$ANTHROPIC_API_KEY:\"; echo sk-ant-api03-abcdefghijklmnopqrstuvwxyz; \
                     echo \"marker:$HORCH_TEST_MARKER\"; echo \"target:$CARGO_TARGET_DIR\"",
                    10_000,
                )],
                a,
                BTreeSet::new(),
            )
            .with_env("ANTHROPIC_API_KEY", "SENTINEL")
            .with_env("HORCH_TEST_MARKER", "set")
        },
    );
    let log = read(&report.gates[0].log_ref);
    assert!(!log.contains("SENTINEL"), "{log}");
    assert!(!log.contains("sk-ant-api03-abc"), "{log}");
    assert_eq!(
        log,
        format!(
            "key::\n[REDACTED]\nmarker:set\ntarget:{}\n",
            worktree.join("target").display()
        )
    );
}
