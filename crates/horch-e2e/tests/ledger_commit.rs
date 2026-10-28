//! `horch ledger commit` at the CLI seam (T-10, decision 01b): the 1 trusted
//! way a sandboxed worker commits. The real `horch` binary, a temporary
//! ledger file and git repository per test.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_e2e::harness::Harness;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let o = as_worker(h, worker, &h.project, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

/// [`ok_as`] from `dir`.
fn ok_in(h: &Harness, worker: &str, dir: &Path, args: &[&str]) -> String {
    let o = as_worker(h, worker, dir, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

/// `horch <args>` as `worker`, from `dir`.
fn as_worker(h: &Harness, worker: &str, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker).current_dir(dir);
    cmd.output().unwrap()
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

/// A run with 1 claimed task and its worktree: the harness, the worker id
/// and the worktree path. The project's git config runs a `pre-commit` hook
/// from the worktree that writes `hook-ran` in the harness root.
fn claimed(name: &str) -> (Harness, String, PathBuf) {
    claimed_after(name, |_| {})
}

/// [`claimed`], with `before` run just before the worktree is made.
fn claimed_after(name: &str, before: impl FnOnce(&Harness)) -> (Harness, String, PathBuf) {
    claimed_in(Harness::new(name).with_git(), before)
}

/// [`claimed_after`] in the project of `h`.
fn claimed_in(mut h: Harness, before: impl FnOnce(&Harness)) -> (Harness, String, PathBuf) {
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let sweep = h.root.join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for r in ["bug-hunter", "clarity-checker"] {
        std::fs::write(
            sweep.join("reviewers").join(format!("{r}.md")),
            format!("You are {r}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("01-a.md");
    std::fs::write(&ticket, "# 01: Feature\n").unwrap();
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &ticket.to_string_lossy(),
            "--primary",
            "opus",
            "--check",
            "true",
        ],
    );
    let w = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    ok_as(&h, &w, &["ledger", "claim"]);
    before(&h);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "feature"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());

    let project = h.project.clone();
    git_in(&h, &project, &["config", "core.hooksPath", "hooks"]);
    std::fs::create_dir_all(path.join("hooks")).unwrap();
    let hook = path.join("hooks/pre-commit");
    let marker = h.root.join("hook-ran");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    (h, w, path)
}

fn head(h: &Harness, dir: &Path) -> String {
    git_in(h, dir, &["rev-parse", "HEAD"])
}

#[test]
fn a_commit_lands_on_the_worktree_branch_without_running_hooks() {
    let (h, w, wt) = claimed("lc-commit");
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    std::fs::write(wt.join("stale.txt"), "stale\n").unwrap();
    git_in(&h, &wt, &["add", "stale.txt"]);
    // A leading dash is a path after `--`, never an option.
    std::fs::write(wt.join("-n.txt"), "dash\n").unwrap();

    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "feature", "--", "a.txt", "-n.txt"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let id = out(&o).trim().to_string();
    assert_eq!(id, head(&h, &wt));
    assert_eq!(
        git_in(&h, &wt, &["branch", "--show-current"]),
        "fleet/alpha-T1-feature"
    );
    let files = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, "-n.txt\na.txt", "only the named files");
    assert_eq!(
        git_in(&h, &wt, &["diff", "--cached", "--name-only"]),
        "stale.txt",
        "a path staged earlier stays out of the commit"
    );
    assert!(
        !h.root.join("hook-ran").exists(),
        "the repository's pre-commit hook must not run"
    );
    let events = ok(&h, &["ledger", "events"]);
    assert!(
        events.contains(&format!("commit {}", &id[..12])),
        "{events}"
    );
}

#[test]
fn paths_outside_the_worktree_or_in_a_nested_repository_are_refused() {
    let (h, w, wt) = claimed("lc-refuse");
    let before = head(&h, &wt);
    std::fs::write(h.root.join("outside.txt"), "x\n").unwrap();
    let outside = h.root.join("outside.txt");
    let nested = wt.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    git_in(&h, &nested, &["init", "--quiet"]);
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();

    for (args, why) in [
        (vec!["--", "../outside.txt"], "is outside the worktree"),
        (
            vec!["--", outside.to_str().unwrap()],
            "is outside the worktree",
        ),
        (vec!["--", "nested/f.txt"], "nested repository"),
        (vec!["--", "nested"], "nested repository"),
    ] {
        let mut all = vec!["ledger", "commit", "-m", "x"];
        all.extend(args.iter().copied());
        let o = as_worker(&h, &w, &wt, &all);
        assert!(!o.status.success(), "{}", text(&o));
        assert!(text(&o).contains(why), "{}", text(&o));
    }
    // From inside the nested repository, too.
    let o = as_worker(
        &h,
        &w,
        &nested,
        &["ledger", "commit", "-m", "x", "--", "f.txt"],
    );
    assert!(text(&o).contains("nested repository"), "{}", text(&o));
    // From outside the worktree.
    let o = as_worker(&h, &w, &h.project, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("inside the worktree"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_worker_without_a_task_or_with_a_review_task_cannot_commit() {
    let (h, w, wt) = claimed("lc-review");
    let idle = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let o = as_worker(&h, &idle, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("has no claimed task"),
        "{}",
        text(&o)
    );

    // A real review task: finish queues reviewer tasks after a commit.
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    // No paths: every change, the hook file too, so the worktree is clean.
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "feature"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(git_in(&h, &wt, &["status", "--porcelain"]).is_empty());
    let o = as_worker(&h, &w, &wt, &["ledger", "finish", "--json"]);
    let r: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert_eq!(r["stopped_at"], "code-sweep", "{r}");

    let rw = ok(
        &h,
        &["ledger", "worker", "new", "--persona", "codex-reviewer"],
    )
    .trim()
    .to_string();
    let c: Value = serde_json::from_str(&ok_as(&h, &rw, &["ledger", "claim", "--json"])).unwrap();
    assert_eq!(c["task"]["kind"], "review", "{c}");
    let before = head(&h, &wt);
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    let o = as_worker(&h, &rw, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("is a review task"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_rewritten_worktree_git_file_is_refused_and_its_config_never_runs() {
    let (h, w, wt) = claimed("lc-gitfile");
    let before = head(&h, &wt);
    // The sandbox can write `<worktree>/.git`: point it at a git dir whose
    // config names a clean filter, as a worker could.
    let evil = wt.join("evil");
    git_in(&h, &wt, &["init", "--quiet", "--bare", "evil"]);
    let marker = h.root.join("filter-ran");
    git_in(
        &h,
        &evil,
        &[
            "config",
            "filter.x.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "* filter=x\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", evil.display())).unwrap();

    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success() && text(&o).contains("was changed"),
        "{}",
        text(&o)
    );
    assert!(!marker.exists(), "the planted filter must not run");
    let main_head = git_in(&h, &h.project, &["rev-parse", "fleet/alpha-T1-feature"]);
    assert_eq!(main_head, before, "nothing is committed");
}

#[test]
fn an_attribute_filter_never_runs_and_the_commit_still_lands() {
    let (h, w, wt) = claimed("lc-filter");
    let marker = h.root.join("filter-ran");
    let project = h.project.clone();
    git_in(
        &h,
        &project,
        &[
            "config",
            "filter.evil.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "* filter=evil\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();

    let o = as_worker(
        &h,
        &w,
        &wt,
        &[
            "ledger",
            "commit",
            "-m",
            "x",
            "--",
            ".gitattributes",
            "a.txt",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "the clean filter must not run");
    let files = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, ".gitattributes\na.txt");
}

#[test]
fn a_symlink_is_committed_as_the_link_and_escapes_through_links_are_refused() {
    let (h, w, wt) = claimed("lc-links");
    std::fs::write(wt.join("target.txt"), "t\n").unwrap();
    std::os::unix::fs::symlink("target.txt", wt.join("link.txt")).unwrap();
    std::fs::write(h.root.join("outside.txt"), "x\n").unwrap();
    std::os::unix::fs::symlink(h.root.join("outside.txt"), wt.join("out-link.txt")).unwrap();
    std::os::unix::fs::symlink(&h.root, wt.join("out-dir")).unwrap();

    let before = head(&h, &wt);
    for (path, why) in [
        ("out-link.txt", "is outside the worktree"),
        ("out-dir/new.txt", "is outside the worktree"),
        (".git", "inside git metadata"),
    ] {
        let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", path]);
        assert!(
            !o.status.success() && text(&o).contains(why),
            "{path}: {}",
            text(&o)
        );
    }
    assert_eq!(head(&h, &wt), before, "nothing is committed");

    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "link", "--", "link.txt"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let tree = git_in(&h, &wt, &["ls-tree", "HEAD", "link.txt"]);
    assert!(tree.starts_with("120000 "), "the link itself: {tree}");
    let files = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, "link.txt", "not its target");
}

#[test]
fn a_stopped_worker_cannot_commit() {
    let (h, w, wt) = claimed("lc-stopped");
    ok(&h, &["ledger", "stop", &w, "test"]);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success() && text(&o).contains("does not own"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_fake_git_on_path_never_runs() {
    let (h, w, wt) = claimed("lc-path");
    let bin = wt.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let marker = h.root.join("fake-git-ran");
    let fake = bin.join("git");
    std::fs::write(&fake, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();

    // No HORCH_GIT_BIN: horch ledger commit takes git from its fixed list.
    let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "a.txt"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env_remove("HORCH_GIT_BIN")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "the git on PATH must not run");
    assert_eq!(out(&o).trim(), head(&h, &wt));

    // A git inside the worktree is refused even when named.
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "b.txt"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env("HORCH_GIT_BIN", &fake)
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(
        !o.status.success()
            && text(&o).contains("inside the worktree, the main checkout or the state root"),
        "{}",
        text(&o)
    );
    assert!(!marker.exists(), "the named fake git must not run");
}

#[test]
fn a_worktree_whose_admin_name_is_taken_still_commits() {
    // Another worktree already holds the admin name T1-feature, so git
    // names this one's entry T1-feature1.
    let (h, w, wt) = claimed_after("lc-collide", |h| {
        let other = h.root.join("elsewhere").join("T1-feature");
        let project = h.project.clone();
        git_in(
            h,
            &project,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "other",
                &other.to_string_lossy(),
            ],
        );
    });
    let project = h.project.clone();
    let common = git_in(
        &h,
        &project,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    assert!(
        Path::new(&common).join("worktrees/T1-feature1").is_dir(),
        "the collision must happen"
    );
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(out(&o).trim(), head(&h, &wt));
}

#[test]
fn a_git_without_attr_source_fails_closed_and_commits_nothing() {
    let (h, w, wt) = claimed("lc-attrsrc");
    let real = h.git_bin().expect("the harness has git").to_path_buf();
    // Reports a new version, but rejects `--attr-source` in `git add` like
    // an older git: the version check passes, and staging must fail closed.
    let fake = h.root.join("old-git");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" add \"*) for a in \"$@\"; do case $a in \
             --attr-source*) echo \"unknown option: $a\" >&2; exit 129;; esac; done;; \
             esac\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);

    let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "a.txt"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env("HORCH_GIT_BIN", &fake)
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(
        !o.status.success()
            && text(&o).contains("git add failed")
            && text(&o).contains("unknown option: --attr-source"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert!(
        git_in(&h, &wt, &["diff", "--cached", "--name-only"]).is_empty(),
        "nothing is staged"
    );
}

#[test]
fn a_path_with_a_required_filter_is_refused_and_the_filter_never_runs() {
    let (h, w, wt) = claimed("lc-lfs");
    let marker = h.root.join("lfs-ran");
    let project = h.project.clone();
    git_in(
        &h,
        &project,
        &[
            "config",
            "filter.lfs.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    git_in(&h, &project, &["config", "filter.lfs.required", "true"]);
    std::fs::write(wt.join(".gitattributes"), "*.bin filter=lfs\n").unwrap();
    std::fs::write(wt.join("big.bin"), "large\n").unwrap();
    let before = head(&h, &wt);
    for args in [
        vec!["ledger", "commit", "-m", "x", "--", "big.bin"],
        vec!["ledger", "commit", "-m", "x"],
    ] {
        let o = as_worker(&h, &w, &wt, &args);
        assert!(
            !o.status.success() && text(&o).contains("big.bin (filter=lfs)"),
            "{}",
            text(&o)
        );
    }
    assert!(!marker.exists(), "the filter must not run");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn hostile_git_variables_from_the_caller_are_ignored() {
    let (h, w, wt) = claimed("lc-env");
    let marker = h.root.join("env-ran");
    let evil = h.root.join("evil.git");
    let root = h.root.clone();
    git_in(
        &h,
        &root,
        &["init", "--quiet", "--bare", &evil.to_string_lossy()],
    );
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let hook = format!("touch '{}'", marker.display());
    let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "a.txt"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env("GIT_DIR", &evil)
        .env("GIT_WORK_TREE", &h.root)
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_0", &hook)
        .env("GIT_CONFIG_KEY_1", "gpg.program")
        .env("GIT_CONFIG_VALUE_1", &hook)
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        out(&o).trim(),
        head(&h, &wt),
        "the commit is in the worktree"
    );
    assert!(!marker.exists(), "no configured program may run");
}

#[test]
fn a_git_older_than_2_41_or_with_bad_version_output_is_refused() {
    let (h, w, wt) = claimed("lc-version");
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    use std::os::unix::fs::PermissionsExt;
    for (name, body) in [
        ("git-2-40", "echo 'git version 2.40.9'"),
        ("git-garbage", "echo 'not a version'"),
        ("git-fails", "exit 1"),
    ] {
        let fake = h.root.join(name);
        std::fs::write(&fake, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "a.txt"]);
        cmd.env("HORCH_WORKER_ID", &w)
            .env("HORCH_GIT_BIN", &fake)
            .current_dir(&wt);
        let o = cmd.output().unwrap();
        assert!(
            !o.status.success() && text(&o).contains("needs git 2.41 or later"),
            "{name}: {}",
            text(&o)
        );
    }
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

/// The git config `filter.lfs.clean` that touches `marker`, and a `*.bin`
/// LFS attribute in the worktree.
fn lfs_filter(h: &Harness, wt: &Path, marker: &Path) {
    let project = h.project.clone();
    git_in(
        h,
        &project,
        &[
            "config",
            "filter.lfs.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "*.bin filter=lfs\n").unwrap();
}

#[test]
fn a_failed_commit_leaves_the_index_as_it_was() {
    let (h, w, wt) = claimed("lc-index");
    std::fs::write(wt.join("stale.txt"), "stale\n").unwrap();
    git_in(&h, &wt, &["add", "stale.txt"]);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    // git refuses an empty commit message after the paths are staged.
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "", "--", "a.txt"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert_eq!(
        git_in(&h, &wt, &["diff", "--cached", "--name-only"]),
        "stale.txt",
        "the index keeps only what was staged before"
    );
    let left: Vec<_> = std::fs::read_dir(h.project.join(".git/worktrees"))
        .unwrap()
        .flat_map(|e| std::fs::read_dir(e.unwrap().path()).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n == "index.horch" || n == "index.lock")
        .collect();
    assert!(
        left.is_empty(),
        "the index copy and lock are removed: {left:?}"
    );
}

#[test]
fn a_folder_or_a_glob_cannot_stage_a_required_filter_path() {
    let (h, w, wt) = claimed("lc-folder");
    let marker = h.root.join("lfs-ran");
    lfs_filter(&h, &wt, &marker);
    let project = h.project.clone();
    git_in(&h, &project, &["config", "filter.lfs.required", "true"]);
    std::fs::create_dir_all(wt.join("assets")).unwrap();
    std::fs::write(wt.join("assets/big.bin"), "large\n").unwrap();
    let before = head(&h, &wt);
    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "x", "--", "assets"],
    );
    assert!(
        !o.status.success() && text(&o).contains("is a folder"),
        "{}",
        text(&o)
    );
    // A glob is a literal name: it matches no file, so nothing is staged.
    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "x", "--", "assets/*.bin"],
    );
    assert!(!o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "the filter must not run");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert_eq!(git_in(&h, &wt, &["diff", "--cached", "--name-only"]), "");
}

#[test]
fn a_required_filter_in_the_global_config_is_refused_too() {
    let (h, w, wt) = claimed("lc-lfs-global");
    let marker = h.root.join("lfs-ran");
    lfs_filter(&h, &wt, &marker);
    // `git lfs install` writes the flag in ~/.gitconfig, not the repository.
    std::fs::write(
        h.home.join(".gitconfig"),
        "[filter \"lfs\"]\n\trequired = true\n",
    )
    .unwrap();
    std::fs::write(wt.join("big.bin"), "large\n").unwrap();
    let before = head(&h, &wt);
    for args in [
        vec!["ledger", "commit", "-m", "x", "--", "big.bin"],
        vec!["ledger", "commit", "-m", "x"],
    ] {
        let o = as_worker(&h, &w, &wt, &args);
        assert!(
            !o.status.success() && text(&o).contains("big.bin (filter=lfs)"),
            "{}",
            text(&o)
        );
    }
    assert!(!marker.exists(), "the filter must not run");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_name_that_is_not_utf8_keeps_its_required_filter() {
    use std::os::unix::ffi::OsStrExt;
    let (h, w, wt) = claimed("lc-bytes");
    let marker = h.root.join("lfs-ran");
    lfs_filter(&h, &wt, &marker);
    let project = h.project.clone();
    git_in(&h, &project, &["config", "filter.lfs.required", "true"]);
    let name = std::ffi::OsStr::from_bytes(b"big-\xff.bin");
    if std::fs::write(wt.join(name), "large\n").is_err() {
        // APFS refuses a name that is not UTF-8: no such file can exist.
        return;
    }
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("(filter=lfs)"),
        "{}",
        text(&o)
    );
    assert!(!marker.exists(), "the filter must not run");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_sha256_repository_commits_without_running_filters() {
    let h = Harness::new("lc-sha256").with_git();
    let project = h.project.clone();
    std::fs::remove_dir_all(project.join(".git")).unwrap();
    for args in [
        &["init", "--quiet", "--object-format=sha256"][..],
        &["symbolic-ref", "HEAD", "refs/heads/main"],
        &["add", "-A"],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "initial commit"],
    ] {
        git_in(&h, &project, args);
    }
    let (h, w, wt) = claimed_in(h, |_| {});
    let marker = h.root.join("filter-ran");
    git_in(
        &h,
        &project,
        &[
            "config",
            "filter.evil.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "* filter=evil\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(o.status.success(), "{}", text(&o));
    let id = out(&o).trim().to_string();
    assert_eq!(id.len(), 64, "a SHA-256 commit id: {id}");
    assert_eq!(id, head(&h, &wt));
    assert!(!marker.exists(), "the clean filter must not run");
}

#[test]
fn a_git_in_the_main_checkout_never_runs() {
    let (h, w, wt) = claimed("lc-git-in-checkout");
    let marker = h.root.join("fake-ran");
    // A pane in the shared checkout can write this file.
    let fake = h.project.join("git");
    std::fs::write(&fake, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", "a.txt"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env("HORCH_GIT_BIN", &fake)
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(
        !o.status.success() && text(&o).contains("the main checkout"),
        "{}",
        text(&o)
    );
    assert!(!marker.exists(), "the planted git must not run");
}

#[test]
fn more_paths_than_the_cap_are_refused() {
    let (h, w, wt) = claimed("lc-cap");
    let dir = wt.join("many");
    std::fs::create_dir_all(&dir).unwrap();
    for n in 0..10_050 {
        std::fs::write(dir.join(format!("{n}.txt")), "").unwrap();
    }
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("at most 10000"),
        "{}",
        text(&o)
    );
    // git is stopped at the cap: horch never counts every change.
    assert!(!text(&o).contains("10050"), "{}", text(&o));
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

/// The worktree's admin dir under the main checkout's `.git/worktrees`.
fn admin_dir(h: &Harness) -> PathBuf {
    let entries: Vec<PathBuf> = std::fs::read_dir(h.project.join(".git/worktrees"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    entries[0].clone()
}

#[test]
fn a_held_index_lock_refuses_the_commit_and_stays() {
    let (h, w, wt) = claimed("lc-lock");
    let lock = admin_dir(&h).join("index.lock");
    std::fs::write(&lock, "").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success() && text(&o).contains("another git command"),
        "{}",
        text(&o)
    );
    assert!(lock.exists(), "the lock of the other command stays");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_required_flag_that_is_not_a_boolean_refuses_the_commit() {
    let (h, w, wt) = claimed("lc-lfs-bad");
    let marker = h.root.join("lfs-ran");
    lfs_filter(&h, &wt, &marker);
    let project = h.project.clone();
    git_in(&h, &project, &["config", "filter.lfs.required", "maybe"]);
    std::fs::write(wt.join("big.bin"), "large\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "x", "--", "big.bin"],
    );
    assert!(
        !o.status.success() && text(&o).contains("cannot read filter.<name>.required"),
        "{}",
        text(&o)
    );
    // With no paths, git status itself refuses the bad value first.
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "the filter must not run");
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_worktree_with_relative_links_still_commits() {
    let (h, w, wt) = claimed_after("lc-relative", |h| {
        let project = h.project.clone();
        git_in(
            h,
            &project,
            &["config", "worktree.useRelativePaths", "true"],
        );
    });
    let dotgit = std::fs::read_to_string(wt.join(".git")).unwrap();
    assert!(
        dotgit.starts_with("gitdir: ../"),
        "git wrote a relative link: {dotgit}"
    );
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(out(&o).trim(), head(&h, &wt));
}

#[test]
fn a_sparse_file_past_the_size_limit_is_refused_before_git_reads_it() {
    let (h, w, wt) = claimed("lc-sparse");
    // 200 MiB of length and no data blocks: only the length tells.
    std::fs::File::create(wt.join("huge.bin"))
        .unwrap()
        .set_len(200 << 20)
        .unwrap();
    let before = head(&h, &wt);
    for args in [
        vec!["ledger", "commit", "-m", "x", "--", "huge.bin"],
        vec!["ledger", "commit", "-m", "x"],
    ] {
        let o = as_worker(&h, &w, &wt, &args);
        assert!(
            !o.status.success() && text(&o).contains("bytes per file"),
            "{}",
            text(&o)
        );
    }
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert_eq!(git_in(&h, &wt, &["diff", "--cached", "--name-only"]), "");
}

#[test]
fn paths_past_the_total_size_limit_are_refused() {
    let (h, w, wt) = claimed("lc-total");
    // 11 sparse files of 99 MiB: each is under the file limit, all are over 1 GiB.
    for n in 0..11 {
        std::fs::File::create(wt.join(format!("part-{n}.bin")))
            .unwrap()
            .set_len(99 << 20)
            .unwrap();
    }
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("more than 1073741824 bytes"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn doctor_warns_about_a_git_inside_the_main_checkout() {
    let (h, _w, _wt) = claimed("lc-doctor");
    let fake = h.project.join("tools-git");
    std::fs::write(&fake, "#!/bin/sh\necho 'git version 2.50.0'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = h.horch(&["doctor"]);
    cmd.env("HORCH_GIT_BIN", &fake).current_dir(&h.project);
    let o = cmd.output().unwrap();
    assert!(
        text(&o).contains("Codex workers cannot commit")
            && text(&o).contains("inside the worktree, the main checkout or the state root"),
        "{}",
        text(&o)
    );
}

/// A nested repository at `wt/<name>` whose own config and
/// `info/attributes` run a clean filter that touches `marker`: the worker
/// can write both. Its file is rewritten after the commit, so a git that
/// checks it hashes it through the filter.
fn nested_repo(h: &Harness, wt: &Path, name: &str, marker: &Path) -> PathBuf {
    let nested = wt.join(name);
    std::fs::create_dir_all(&nested).unwrap();
    git_in(h, &nested, &["init", "--quiet"]);
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();
    git_in(h, &nested, &["add", "f.txt"]);
    git_in(h, &nested, &["commit", "--quiet", "-m", "n"]);
    let clean = format!("touch '{}'; cat", marker.display());
    git_in(h, &nested, &["config", "filter.evil.clean", &clean]);
    std::fs::write(nested.join(".git/info/attributes"), "* filter=evil\n").unwrap();
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();
    nested
}

#[test]
fn an_untracked_nested_repository_is_never_staged_as_a_gitlink() {
    let (h, w, wt) = claimed("lc-gitlink-new");
    let marker = h.root.join("filter-ran");
    nested_repo(&h, &wt, "inner", &marker);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("inner/ is a nested repository"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert!(!marker.exists(), "the nested filter must not run");
}

#[test]
fn a_tracked_gitlink_starts_no_git_in_it_and_is_never_staged() {
    let (h, w, wt) = claimed("lc-gitlink-old");
    let marker = h.root.join("filter-ran");
    let nested = nested_repo(&h, &wt, "inner", &marker);
    // The operator committed the gitlink with their own git.
    git_in(&h, &wt, &["add", "inner"]);
    git_in(
        &h,
        &wt,
        &["commit", "--quiet", "--no-verify", "-m", "gitlink"],
    );
    let gitlink = git_in(&h, &wt, &["rev-parse", "HEAD:inner"]);
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();
    let _ = std::fs::remove_file(&marker);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        !marker.exists(),
        "no git may start in the nested repository"
    );
    let names = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(names.lines().any(|n| n == "a.txt"), "{names}");

    // A new commit in the nested repository would move the gitlink: it is
    // not staged, and the other change still commits.
    std::fs::write(nested.join("g.txt"), "g\n").unwrap();
    git_in(&h, &nested, &["add", "g.txt"]);
    git_in(&h, &nested, &["commit", "--quiet", "-m", "g"]);
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();
    let _ = std::fs::remove_file(&marker);
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "y"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        !marker.exists(),
        "no git may start in the nested repository"
    );
    assert_eq!(git_in(&h, &wt, &["rev-parse", "HEAD:inner"]), gitlink);
    assert_eq!(
        git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]),
        "b.txt"
    );
}

#[test]
fn a_lock_left_by_an_ended_commit_is_named_as_stale() {
    let (h, w, wt) = claimed("lc-stale-lock");
    let lock = admin_dir(&h).join("index.lock");
    let mut ended = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = ended.id();
    ended.wait().unwrap();
    std::fs::write(&lock, format!("horch ledger commit pid {pid}\n")).unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success()
            && text(&o).contains(&format!("{} is stale", lock.display()))
            && text(&o).contains(&format!("pid {pid}")),
        "{}",
        text(&o)
    );
    assert!(lock.exists(), "horch never removes a lock it did not make");
}

#[test]
fn a_scratch_index_lock_left_by_a_killed_git_does_not_block_the_next_commit() {
    let (h, w, wt) = claimed("lc-scratch-lock");
    let admin = admin_dir(&h);
    std::fs::write(admin.join("index.horch"), "partial").unwrap();
    std::fs::write(admin.join("index.horch.lock"), "partial").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!admin.join("index.horch.lock").exists());
    assert!(!admin.join("index.lock").exists());
}

#[test]
fn the_author_is_the_repository_identity_then_the_global_one() {
    let (h, w, wt) = claimed("lc-identity");
    std::fs::write(
        h.home.join(".gitconfig"),
        "[user]\n\tname = Global Op\n\temail = global@example.com\n",
    )
    .unwrap();
    let commit_as = |file: &str| {
        std::fs::write(wt.join(file), "x\n").unwrap();
        let mut cmd = h.horch(&["ledger", "commit", "-m", "x", "--", file]);
        cmd.env("HORCH_WORKER_ID", &w).current_dir(&wt);
        for var in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_AUTHOR_DATE",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "GIT_COMMITTER_DATE",
        ] {
            cmd.env_remove(var);
        }
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        git_in(&h, &wt, &["log", "-1", "--format=%an <%ae>"])
    };
    assert_eq!(commit_as("a.txt"), "Global Op <global@example.com>");
    let project = h.project.clone();
    git_in(&h, &project, &["config", "user.name", "Repo Op"]);
    git_in(&h, &project, &["config", "user.email", "repo@example.com"]);
    assert_eq!(commit_as("b.txt"), "Repo Op <repo@example.com>");
}

#[test]
fn a_path_that_becomes_a_nested_repository_after_the_checks_is_never_committed() {
    let (h, w, wt) = claimed("lc-race");
    std::fs::write(wt.join("d"), "d\n").unwrap();
    git_in(&h, &wt, &["add", "d"]);
    git_in(&h, &wt, &["commit", "--quiet", "--no-verify", "-m", "d"]);
    std::fs::remove_file(wt.join("d")).unwrap();
    let marker = h.root.join("monitor-ran");
    let monitor = h.root.join("monitor");
    std::fs::write(
        &monitor,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    // Between the checks and git add, the deleted path becomes a nested
    // repository whose fsmonitor a later plain git status would run.
    let real = h.git_bin().expect("the harness has git").to_path_buf();
    let nested = wt.join("d");
    let swap = format!(
        "g() {{ env -i PATH=/usr/bin:/bin '{real}' -c user.name=x -c user.email=x@x \"$@\"; }}\n\
         g init -q '{d}' && echo n > '{d}/n' && g -C '{d}' add n && g -C '{d}' commit -qm n \
         && g -C '{d}' config core.fsmonitor '{monitor}'",
        real = real.display(),
        d = nested.display(),
        monitor = monitor.display()
    );
    let fake = h.root.join("racing-git");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" check-attr \"*) {swap} >/dev/null 2>&1;; esac\n\
             exec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    for f in [&fake, &monitor] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let before = head(&h, &wt);
    let mut cmd = h.horch(&["ledger", "commit", "-m", "x"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .env("HORCH_GIT_BIN", &fake)
        .current_dir(&wt);
    let o = cmd.output().unwrap();
    assert!(nested.join(".git").exists(), "the race ran");
    assert!(
        !o.status.success() && text(&o).contains("d became a gitlink"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    // The plain git status that horch ledger finish runs.
    git_in(&h, &wt, &["status", "--porcelain"]);
    assert!(
        !marker.exists(),
        "no git may start in the nested repository"
    );
}

#[test]
fn with_nothing_listed_no_git_add_runs_and_a_moved_gitlink_stays_out() {
    let (h, w, wt) = claimed("lc-empty");
    let marker = h.root.join("filter-ran");
    let nested = nested_repo(&h, &wt, "inner", &marker);
    git_in(&h, &wt, &["add", "inner"]);
    git_in(
        &h,
        &wt,
        &["commit", "--quiet", "--no-verify", "-m", "gitlink"],
    );
    let gitlink = git_in(&h, &wt, &["rev-parse", "HEAD:inner"]);
    // Commit every other change, so git status lists nothing after it.
    ok_in(&h, &w, &wt, &["ledger", "commit", "-m", "rest"]);
    git_in(
        &h,
        &nested,
        &["commit", "--quiet", "--allow-empty", "-m", "moved"],
    );
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();
    let _ = std::fs::remove_file(&marker);
    let before = head(&h, &wt);
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert_eq!(head(&h, &wt), before, "nothing is committed");
    assert_eq!(git_in(&h, &wt, &["rev-parse", "HEAD:inner"]), gitlink);
    assert!(
        !marker.exists(),
        "no git may start in the nested repository"
    );
}

#[test]
fn every_kind_of_change_commits_with_no_paths() {
    let (h, w, wt) = claimed("lc-kinds");
    for f in ["mod.txt", "del.txt", "gone.txt", "old.txt"] {
        std::fs::write(wt.join(f), "1\n").unwrap();
    }
    git_in(
        &h,
        &wt,
        &["add", "mod.txt", "del.txt", "gone.txt", "old.txt"],
    );
    git_in(&h, &wt, &["commit", "--quiet", "--no-verify", "-m", "base"]);
    std::fs::write(wt.join("mod.txt"), "2\n").unwrap();
    std::fs::remove_file(wt.join("del.txt")).unwrap();
    // A staged deletion: in neither the index nor the worktree.
    git_in(&h, &wt, &["rm", "--quiet", "gone.txt"]);
    git_in(&h, &wt, &["mv", "old.txt", "new.txt"]);
    std::fs::write(wt.join("sp ace.txt"), "s\n").unwrap();
    std::fs::write(wt.join("line\nbreak.txt"), "n\n").unwrap();
    ok_in(&h, &w, &wt, &["ledger", "commit", "-m", "kinds"]);
    assert_eq!(git_in(&h, &wt, &["status", "--porcelain"]), "");
    let changed = git_in(
        &h,
        &wt,
        &[
            "show",
            "--name-status",
            "--no-renames",
            "-z",
            "--format=",
            "HEAD",
        ],
    );
    for entry in [
        "M\0mod.txt",
        "D\0del.txt",
        "D\0gone.txt",
        "D\0old.txt",
        "A\0new.txt",
        "A\0sp ace.txt",
        "A\0line\nbreak.txt",
    ] {
        assert!(changed.contains(entry), "{entry:?} in {changed:?}");
    }
}

#[test]
fn a_lock_of_a_running_process_is_not_called_stale() {
    let (h, w, wt) = claimed("lc-live-lock");
    let lock = admin_dir(&h).join("index.lock");
    std::fs::write(
        &lock,
        format!("horch ledger commit pid {}\n", std::process::id()),
    )
    .unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success()
            && text(&o).contains("another git command")
            && !text(&o).contains("is stale"),
        "{}",
        text(&o)
    );
    assert!(lock.exists(), "the lock stays");
}
