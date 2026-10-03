//! End-to-end: `horch skills install` pins a git skill from a local bare
//! repository, a repeat install changes nothing, `update` follows a moved
//! branch, and `doctor` verifies the store. Real git runs on temp repos only.

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_e2e::harness::Harness;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let out = h.run(args);
    assert!(
        out.status.success(),
        "horch {args:?} failed: {}",
        text(&out)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn git(h: &Harness, dir: &Path, args: &[&str]) {
    let mut full = vec!["-C", dir.to_str().unwrap()];
    full.extend_from_slice(args);
    let out = h.git_cmd(&full).output().expect("running git");
    assert!(out.status.success(), "git {args:?} failed: {}", text(&out));
}

/// A bare repository `remote.git` on branch `main` with one valid skill
/// at `skills/demo/`, and the work tree that pushes to it.
fn fixture(h: &Harness) -> (PathBuf, PathBuf) {
    let bare = h.root.join("remote.git");
    let work = h.root.join("upstream");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::create_dir_all(work.join("skills/demo")).unwrap();
    git(h, &bare, &["init", "--quiet", "--bare"]);
    git(h, &bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(h, &work, &["init", "--quiet"]);
    git(h, &work, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    write_skill(&work, "first body");
    commit_and_push(h, &work, &bare, "first");
    (bare, work)
}

fn write_skill(work: &Path, body: &str) {
    std::fs::write(
        work.join("skills/demo/SKILL.md"),
        format!(
            "---\nname: demo\ndescription: A demo skill for the marketplace e2e.\n---\n{body}\n"
        ),
    )
    .unwrap();
}

fn commit_and_push(h: &Harness, work: &Path, bare: &Path, msg: &str) {
    git(h, work, &["add", "-A"]);
    git(h, work, &["commit", "--quiet", "--no-gpg-sign", "-m", msg]);
    git(
        h,
        work,
        &["push", "--quiet", bare.to_str().unwrap(), "main:main"],
    );
}

fn lock(h: &Harness) -> Value {
    let path = h.home.join(".local/share/horch/marketplace.lock");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn versions_on_disk(h: &Harness) -> Vec<String> {
    let dir = h.home.join(".local/share/horch/skills/demo");
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn mkt_08_e2e_install_update_repeatable() {
    let mut h = Harness::new("mkt08").with_git();
    if h.git_bin().is_none() {
        return;
    }
    let (bare, work) = fixture(&h);
    let source = format!("file://{}@main", bare.display());

    // Install pins the full commit and names the version after it.
    let first = ok(&h, &["skills", "install", &source, "--path", "skills/demo"]);
    let entry = lock(&h)["skills"][0].clone();
    let commit = entry["resolved_commit"].as_str().unwrap().to_owned();
    assert_eq!(commit.len(), 40, "{entry}");
    assert!(commit.bytes().all(|c| c.is_ascii_hexdigit()), "{entry}");
    assert_eq!(entry["id"], "demo");
    assert_eq!(entry["requested_revision"], "main");
    assert_eq!(entry["version"], format!("git+{}", &commit[..12]));
    assert!(first.contains(&commit), "{first}");

    // The same spec again is a no-op: same pin, same digest, one version.
    let again = ok(&h, &["skills", "install", &source, "--path", "skills/demo"]);
    assert!(again.starts_with("unchanged demo"), "{again}");
    let repeat = lock(&h)["skills"][0].clone();
    for key in [
        "source",
        "requested_revision",
        "resolved_commit",
        "version",
        "digest",
    ] {
        assert_eq!(repeat[key], entry[key], "{key}");
    }
    assert_eq!(
        versions_on_disk(&h),
        vec![entry["version"].as_str().unwrap()]
    );

    // Moving the branch and running update installs the new commit.
    write_skill(&work, "second body");
    commit_and_push(&h, &work, &bare, "second");
    let updated = ok(&h, &["skills", "update", "demo"]);
    let moved = lock(&h)["skills"][0].clone();
    assert_ne!(moved["resolved_commit"], entry["resolved_commit"]);
    assert_ne!(moved["version"], entry["version"]);
    assert_ne!(moved["digest"], entry["digest"]);
    assert_eq!(
        updated.trim(),
        format!(
            "demo {} -> {}",
            entry["version"].as_str().unwrap(),
            moved["version"].as_str().unwrap()
        )
    );

    // The store verifies, with no git at all.
    h.set("HORCH_GIT_BIN", "/nonexistent");
    let doctor = ok(&h, &["skills", "doctor"]);
    assert!(
        doctor.contains("1 skill(s) checked, 0 problem(s)"),
        "{doctor}"
    );

    // A changed file fails doctor with exit 1.
    let installed = h
        .home
        .join(".local/share/horch/skills/demo")
        .join(moved["version"].as_str().unwrap())
        .join("SKILL.md");
    std::fs::write(&installed, "tampered\n").unwrap();
    let out = h.run(&["skills", "doctor"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("changed  demo"));
}
