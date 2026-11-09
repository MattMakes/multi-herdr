//! A git network call against a remote that never answers ends within its
//! bound, and the result names the command and the bound (ticket
//! quiet-orchestrator-mail/03).
#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use horch_core::taskledger::git::Git;

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A repository whose `origin` goes through an ssh that never answers.
fn stalled_repo(tmp: &Path) -> std::path::PathBuf {
    let ssh = tmp.join("stall-ssh");
    std::fs::write(&ssh, "#!/bin/sh\nexec sleep 600\n").unwrap();
    horch_core::runtime::process::make_executable(&ssh).unwrap();
    let repo = tmp.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    git(&repo, &["config", "core.sshCommand", ssh.to_str().unwrap()]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "x",
        ],
    );
    git(&repo, &["remote", "add", "origin", "stalled.invalid:x.git"]);
    repo
}

#[test]
fn a_stalled_remote_fails_within_the_bound() {
    // The only test in this binary, so no other test sees the short bound.
    horch_core::vcs::network::set_test_bound(Duration::from_millis(1500));
    let tmp = tempfile::tempdir().unwrap();
    let repo = stalled_repo(tmp.path());
    for args in [
        &["fetch", "origin", "main"][..],
        &["ls-remote", "origin", "refs/heads/main"][..],
        &["push", "origin", "main:main"][..],
    ] {
        let (tx, rx) = mpsc::channel();
        let (r, a) = (
            repo.clone(),
            args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        );
        let started = Instant::now();
        std::thread::spawn(move || {
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            let _ = tx.send(Git::new("git").try_network(&r, &a));
        });
        let out = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| panic!("git {args:?} hung past 30 s"))
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(15), "{args:?}");
        assert!(!out.ok, "{args:?}");
        let want = format!("git {} ran past its bound of 1.5 s", args.join(" "));
        assert!(out.stderr.contains(&want), "{args:?}: {}", out.stderr);
    }
}
