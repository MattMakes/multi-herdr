//! `horch ledger handoff` (trace-fixes C4, TRF-37): the worker ends handed
//! off, its task is queued again first, and the next claim prints
//! `PRIOR WORK: read <path> first` before the ticket snapshot. The test runs
//! the binary on a temp git project, with the watchdog and wakes off and no
//! herdr pane.

use std::path::PathBuf;
use std::process::Command;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

struct World {
    tmp: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(proj.join("tickets")).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["commit", "-q", "--allow-empty", "-m", "init"][..],
        ] {
            let st = Command::new("git")
                .args(args)
                .current_dir(&proj)
                .env_clear()
                .env("PATH", PATH)
                .env("HOME", tmp.path().join("home"))
                .env("GIT_AUTHOR_NAME", "n")
                .env("GIT_AUTHOR_EMAIL", "a@b")
                .env("GIT_COMMITTER_NAME", "n")
                .env("GIT_COMMITTER_EMAIL", "a@b")
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        }
        World { tmp }
    }

    fn proj(&self) -> PathBuf {
        self.tmp.path().join("proj")
    }

    /// Run horch; `worker` sets `HORCH_WORKER_ID`. Returns code, stdout, stderr.
    fn run(&self, worker: Option<&str>, args: &[&str]) -> (i32, String, String) {
        let root = self.tmp.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_NO_WATCHDOG", "1")
            .env("HORCH_NO_WAKE", "1");
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        let o = cmd.output().unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    }

    fn ok(&self, worker: Option<&str>, args: &[&str]) -> String {
        let (code, out, err) = self.run(worker, args);
        assert_eq!(code, 0, "{args:?}: {out}{err}");
        out
    }

    fn hire(&self) -> String {
        self.ok(None, &["ledger", "worker", "new", "--persona", "dev"])
            .trim()
            .to_string()
    }
}

const TICKET: &str = "# 01: A\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`\n\n## Goal\n\nDo A.\n";

const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [A](a.md) | | 1 |\n";

#[test]
fn trf_37_handoff_requeues_the_task_and_the_next_claim_reads_the_handoff_first() {
    let w = World::new();
    w.ok(None, &["ledger", "run", "new", "r1"]);
    std::fs::write(w.proj().join("tickets/a.md"), TICKET).unwrap();
    std::fs::write(w.proj().join("tickets/MAP.md"), MAP).unwrap();
    w.ok(None, &["ledger", "add-map", "tickets/MAP.md"]);
    let first = w.hire();
    w.ok(Some(&first), &["ledger", "claim"]);

    // A missing handoff file is refused, and the task stays with the worker.
    let (code, _, err) = w.run(Some(&first), &["ledger", "handoff", "ai_docs/h.md"]);
    assert_ne!(code, 0);
    assert!(err.contains("does not exist"), "{err}");

    let file = w.proj().join("ai_docs/h.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "# Handoff\n").unwrap();
    let out = w.ok(Some(&first), &["ledger", "handoff", "ai_docs/h.md"]);
    assert!(out.contains("T-1 is handed off."), "{out}");

    let show: serde_json::Value =
        serde_json::from_str(&w.ok(None, &["ledger", "show", "T-1", "--json"])).unwrap();
    let status = show["task"]["status"]
        .as_str()
        .or_else(|| show["status"].as_str());
    assert_eq!(status, Some("queued"), "{show}");

    let next = w.hire();
    let out = w.ok(Some(&next), &["ledger", "claim"]);
    let abs = file.canonicalize().unwrap();
    let prior = format!("PRIOR WORK: read {} first", abs.display());
    let prior_at = out
        .find(&prior)
        .unwrap_or_else(|| panic!("{prior} in:\n{out}"));
    let ticket_at = out.find("Ticket snapshot").expect("the snapshot");
    assert!(prior_at < ticket_at, "PRIOR WORK comes first:\n{out}");
    assert!(out.contains("Claimed T-1 (attempt 2)"), "{out}");
}
