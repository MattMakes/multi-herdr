//! Remaining fleet briefing and worktree contracts at the public seam.
use horch_core::{execution::SessionMode, prompts, roster::Roster};
use horch_e2e::harness::Harness;
fn worker() -> String {
    let r = Roster::builtin().unwrap();
    prompts::worker_prompt(
        &r,
        r.require("sonnet").unwrap(),
        "s-1",
        "test",
        &SessionMode::Fresh(None),
    )
    .unwrap()
}
#[test]
fn g56_fact_command_accepts_and_deduplicates() {
    let h = Harness::new("g56-facts").with_git();
    for _ in 0..2 {
        let o = h.run(&["note", "--fact", "Use BSD sed"]);
        assert!(o.status.success(), "{o:?}");
    }
}
#[test]
fn g57_orchestrator_resume_has_command_card() {
    let r = Roster::builtin().unwrap();
    let p = prompts::context_message(
        &r,
        "resume-orchestrator",
        &[("handoff", "handoff.md")].into(),
    )
    .unwrap();
    assert!(p.contains("horch note --fact"));
    assert!(!p.contains('\n'));
}
#[test]
fn g58_codex_description_states_current_boundaries() {
    let r = Roster::builtin().unwrap();
    for n in r.names() {
        let t = r.require(n).unwrap();
        if t.agent.as_str() == "codex" {
            assert!(t.brief_description.contains(".git"), "{n}");
        }
    }
}
#[test]
fn g63_worker_has_questions_background_and_one_done() {
    let p = worker();
    for s in [
        "reversible",
        "waiting: <what>",
        "[crushed]",
        "exact command",
        "every ticket step",
        "NOTE: The report",
    ] {
        assert!(p.contains(s), "missing {s}");
    }
    assert!(!p.contains("or background tasks"));
}
#[test]
fn g64_orchestrator_delegates_verification() {
    let r = Roster::builtin().unwrap();
    let p = prompts::agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
    assert!(p.contains("verified"));
    assert!(p.contains("horch ledger steps"));
    assert!(!p.contains("Run the\n   tests the plan names"));
}
#[test]
fn g69_worktree_new_has_from_flag() {
    let h = Harness::new("g69-help");
    let o = h.run(&["ledger", "worktree", "new", "--help"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("--from"));
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "{args:?}: {o:?}");
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn as_worker(h: &Harness, w: &str, args: &[&str]) -> std::process::Output {
    h.horch(args).env("HORCH_WORKER_ID", w).output().unwrap()
}
fn worker_ok(h: &Harness, w: &str, args: &[&str]) -> String {
    let o = as_worker(h, w, args);
    assert!(o.status.success(), "{o:?}");
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn ledger_world(name: &str) -> (Harness, String) {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    ok(&h, &["ledger", "run", "new", "review"]);
    std::fs::write(h.project.join("ticket.md"), "# Review\n").unwrap();
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            "ticket.md",
            "--primary",
            "sonnet",
        ],
    );
    let w = ok(&h, &["ledger", "worker", "new", "--persona", "sonnet"])
        .trim()
        .to_owned();
    worker_ok(&h, &w, &["ledger", "claim"]);
    (h, w)
}
fn git(h: &Harness, dir: &std::path::Path, args: &[&str]) -> String {
    let o = h.git_cmd(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "{o:?}");
    String::from_utf8_lossy(&o.stdout).trim().into()
}
fn tree_path(text: &str) -> std::path::PathBuf {
    text.lines()
        .find_map(|l| l.strip_prefix("path: "))
        .unwrap()
        .into()
}
#[test]
fn g56_facts_reach_claim_and_launch_context_and_are_project_scoped() {
    let (h, w) = ledger_world("g56-context");
    worker_ok(&h, &w, &["note", "--fact", "Use BSD sed"]);
    worker_ok(&h, &w, &["note", "--fact", "Use BSD sed"]);
    let facts = horch_core::project_notes::read(&h.state, &h.project).unwrap();
    assert_eq!(facts, ["Use BSD sed"]);
    let other = h.root.join("other-project");
    std::fs::create_dir(&other).unwrap();
    assert!(horch_core::project_notes::read(&h.state, &other)
        .unwrap()
        .is_empty());
    // Claim is idempotent for this worker and prints project knowledge each time.
    let claim = worker_ok(&h, &w, &["ledger", "claim"]);
    assert!(claim.contains("Use BSD sed"), "{claim}");
    let r = Roster::builtin().unwrap();
    let host = horch_core::project_notes::host_line();
    let display = horch_core::project_notes::display(&h.state, &h.project).unwrap();
    let p = prompts::worker_prompt_with_context(
        &r,
        r.require("sonnet").unwrap(),
        "s-1",
        "test",
        &SessionMode::Fresh(None),
        &display,
        &host,
    )
    .unwrap();
    assert!(p.contains("Use BSD sed"));
    assert!(p.contains(std::env::consts::OS));
    assert!(p.contains(std::env::consts::ARCH));
}
#[test]
fn g69_from_scratch_and_drop_preserve_unpublished_work() {
    let (h, w) = ledger_world("g69-ops");
    // The PR ref must differ from the run branch: ignoring --from fails this test.
    std::fs::write(h.project.join("pr-only.txt"), "PR branch content\n").unwrap();
    git(&h, &h.project, &["add", "pr-only.txt"]);
    git(&h, &h.project, &["commit", "-q", "-m", "PR branch"]);
    let base = git(&h, &h.project, &["rev-parse", "HEAD"]);
    git(&h, &h.project, &["branch", "pr-review"]);
    let made = worker_ok(
        &h,
        &w,
        &["ledger", "worktree", "new", "review", "--from", "pr-review"],
    );
    let path = tree_path(&made);
    assert_eq!(git(&h, &path, &["rev-parse", "HEAD"]), base);
    ok(&h, &["note", "--fact", "Shared across linked checkouts"]);
    assert_eq!(
        horch_core::project_notes::read(&h.state, &path).unwrap(),
        ["Shared across linked checkouts"]
    );
    let status = String::from_utf8(h.run(&["ledger", "status", "--json"]).stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(v["worktrees"][0]["base"], "pr-review");
    std::fs::write(path.join("unique.txt"), "new work\n").unwrap();
    git(&h, &path, &["add", "unique.txt"]);
    git(&h, &path, &["commit", "-q", "-m", "unique"]);
    let branch = git(&h, &path, &["branch", "--show-current"]);
    let denied = as_worker(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            "review",
            "--reason",
            "stop review",
        ],
    );
    assert!(!denied.status.success());
    assert!(path.exists());
    assert!(git(&h, &h.project, &["branch", "--list", &branch]).contains(&branch));
    let before: serde_json::Value =
        serde_json::from_slice(&h.run(&["ledger", "status", "--json"]).stdout).unwrap();
    assert_eq!(before["worktrees"][0]["status"], "open");
    assert!(before["worktrees"][0]["reason"].is_null());
    worker_ok(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            "review",
            "--reason",
            "stop review",
            "--keep-branch",
        ],
    );
    assert!(!path.exists());
    assert!(git(&h, &h.project, &["branch", "--list", &branch]).contains(&branch));
    let st = String::from_utf8(h.run(&["ledger", "status", "--json"]).stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&st).unwrap();
    assert_eq!(v["worktrees"][0]["status"], "abandoned");
    assert_eq!(v["worktrees"][0]["reason"], "stop review");
    let scratch = worker_ok(&h, &w, &["ledger", "worktree", "scratch", "inspect"]);
    let path = tree_path(&scratch);
    assert!(path
        .to_string_lossy()
        .contains(".worktrees/_scratch/inspect"));
    let s = horch_core::taskledger::store::Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    let claim = worker_ok(&h, &w, &["ledger", "claim", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&claim).unwrap();
    let task = v["task"]["task_id"].as_str().unwrap();
    assert!(horch_core::taskledger::finish::worktree_of(s.conn(), task)
        .unwrap()
        .is_none());
    // Dirt is always preserved, including when keeping the branch.
    std::fs::write(path.join("dirty.txt"), "keep me").unwrap();
    assert!(!as_worker(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            "inspect",
            "--reason",
            "done",
            "--keep-branch"
        ]
    )
    .status
    .success());
    assert!(path.join("dirty.txt").is_file());
}

#[test]
fn g57_overlay_missing_orchestrator_resume_fails_check() {
    let dir = tempfile::tempdir().unwrap();
    let bases = dir.path().join("_base");
    std::fs::create_dir(&bases).unwrap();
    let original = std::fs::read_to_string(
        horch_e2e::harness::repo_root().join("teammates/_base/context-messages.md"),
    )
    .unwrap();
    let without = original
        .lines()
        .filter(|l| !l.starts_with("  resume-orchestrator:"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(bases.join("context-messages.md"), without).unwrap();
    let r = Roster::load_layered(None, None, dir.path().to_str()).unwrap();
    assert!(
        r.check().iter().any(|s| s.contains("resume-orchestrator")),
        "{:?}",
        r.check()
    );
}
#[test]
fn g56_facts_expire_oldest_and_refuse_multiline() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let state = dir.path().join("state");
    for i in 0..105 {
        assert!(horch_core::project_notes::record(&state, &project, &format!("fact {i}")).unwrap());
    }
    let facts = horch_core::project_notes::read(&state, &project).unwrap();
    assert_eq!(facts.len(), 100);
    assert_eq!(facts.first().unwrap(), "fact 5");
    assert_eq!(facts.last().unwrap(), "fact 104");
    assert!(horch_core::project_notes::record(&state, &project, "two\nlines").is_err());
}
#[test]
fn g56_later_spawn_receives_recorded_fact() {
    let mut h = Harness::new("g56-spawn").with_git();
    h.set(
        "HORCH_QUOTA_FILE",
        horch_e2e::harness::fixtures()
            .join("quota/all-ok.json")
            .to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    assert!(create
        .args(["workspace", "create", "--label", "facts"])
        .output()
        .unwrap()
        .status
        .success());
    ok(&h, &["note", "--fact", "Use portable BSD sed"]);
    ok(
        &h,
        &[
            "spawn",
            "sonnet",
            "--from-pane",
            "w1:p1",
            "--force",
            "--no-tile",
            "Read the project facts",
        ],
    );
    let call = horch_e2e::harness::wait_for("worker brief with project facts", || {
        h.calls_of("claude")
            .into_iter()
            .find(|c| c.to_string().contains("Use portable BSD sed"))
    });
    let prompt = call.to_string();
    assert!(prompt.contains(std::env::consts::OS));
    assert!(prompt.contains(std::env::consts::ARCH));
}
#[test]
fn g69_open_scratch_does_not_block_finish_or_done() {
    let (h, w) = ledger_world("g69-scratch-done");
    worker_ok(&h, &w, &["ledger", "worktree", "new", "task"]);
    let made = worker_ok(&h, &w, &["ledger", "worktree", "scratch", "inspect"]);
    let path = tree_path(&made);
    // Select just the check step for this run: no task check means N/A. This
    // exercises actual finish and done without an unrelated PR remote or sweep.
    let mut s = horch_core::taskledger::store::Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    s.immediate(|tx| {
        tx.execute("UPDATE runs SET finish_steps='[\"check\"]'", [])?;
        Ok(())
    })
    .unwrap();
    worker_ok(&h, &w, &["ledger", "finish"]);
    worker_ok(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            "T1-task",
            "--reason",
            "No changes needed",
        ],
    );
    worker_ok(&h, &w, &["ledger", "done", "Reviewed in scratch"]);
    assert!(path.exists());
    let o = h.run(&["ledger", "status", "--json"]);
    assert!(o.status.success(), "{o:?}");
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["complete"], true);
}

#[test]
fn g64_skill_states_each_delegation_and_reporting_rule_within_budget() {
    let s = std::fs::read_to_string(
        horch_e2e::harness::repo_root().join("skills/orchestrate/SKILL.md"),
    )
    .unwrap();
    assert!(s.len() <= 12288);
    for required in [
        "verified (with evidence) or not verified",
        "failed commands and deliveries as failed",
        "Your own checks only read",
        "Delegate landing, installs, cleanup",
        "Do not re-run the tests",
        "1 full gate on the run branch to 1 worker",
        "1 unit per item",
        "active runs",
        "live orchestrators",
        "open project PRs",
        "horch peek",
        "only DONE path",
    ] {
        assert!(s.contains(required), "missing {required}");
    }
    for forbidden in [
        "Land the branch, or",
        "Run the full verification yourself",
        "when to send `horch tell orchestrator`",
        "Send DONE: when the tests pass",
    ] {
        assert!(!s.contains(forbidden), "{forbidden}");
    }
}
#[test]
fn g63_warning_is_one_line_and_offers_finish_when_done() {
    let r = Roster::builtin().unwrap();
    let vars = [
        ("role", "s-1"),
        ("tokens", "100"),
        ("threshold", "90"),
        ("handoff", "handoff.md"),
    ]
    .into();
    let p = prompts::context_message(&r, "warning", &vars).unwrap();
    assert!(!p.contains('\n'));
    assert!(p.contains("If DONE WHEN is met, run horch ledger finish, then horch ledger done (or horch done) instead."));
}

#[test]
fn g57_compacting_orchestrator_types_its_command_card() {
    use horch_core::execution::records::SessionLedger;
    let mut h = Harness::new("g57-compact").with_git();
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.copy_in(
        &horch_e2e::harness::repo_root().join("crates/horch-core/tests/fixtures/context/home"),
        &h.home,
    );
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    assert!(cmd
        .args(["workspace", "create", "--label", "resume"])
        .output()
        .unwrap()
        .status
        .success());
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    assert!(cmd
        .args([
            "pane",
            "test-state",
            "w1:p1",
            "--status",
            "idle",
            "--agent",
            "claude"
        ])
        .output()
        .unwrap()
        .status
        .success());
    let mailbox = horch_core::messaging::mailbox::Mailbox::under(&h.state, "w1");
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    std::fs::write(mailbox.dir().join("orchestrator.id"), "w1:p1").unwrap();
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    ledger
        .add(horch_core::execution::records::NewRecord {
            record_id: "r-orchestrator",
            agent: "claude",
            tier: "orchestrator",
            model: "fable",
            role: "orchestrator",
            session_id: Some("11111111-1111-4111-8111-111111111111"),
            task: "run",
            phase: None,
        })
        .unwrap();
    ledger
        .store()
        .update(|rows| {
            let r = rows
                .iter_mut()
                .find(|r| r.record_id == "r-orchestrator")
                .unwrap();
            r.kind = "orchestrator".into();
            r.workspace_id = Some("w1".into());
            r.pane_id = Some("w1:p1".into());
            Ok(())
        })
        .unwrap();
    h.set("HORCH_RECORD_ID", "r-orchestrator");
    h.set("HORCH_ROLE", "orchestrator");
    let ctx = horch_core::runtime::RuntimeContext::from_env(
        &horch_core::runtime::MapEnv::new(&h.project).with("HOME", &h.home.to_string_lossy()),
    )
    .unwrap();
    let loc = horch_core::usage::Locations::from_context(&ctx);
    let transcript = horch_core::telemetry::context::read_current(
        &loc,
        &h.state.join("context"),
        "claude",
        Some("11111111-1111-4111-8111-111111111111"),
    )
    .unwrap()
    .transcript;
    h.set(
        "HORCH_FAKE_COMPACT_TRANSCRIPT",
        transcript.to_string_lossy(),
    );
    let handoff = h
        .project
        .join("ai_docs/handoffs/orchestrator-whats-next.md");
    std::fs::create_dir_all(handoff.parent().unwrap()).unwrap();
    std::fs::write(
        handoff,
        "# Handoff\nRun slug: resume\nRead ai_docs/plans/resume/decisions.md.\n",
    )
    .unwrap();
    let response = ok(&h, &["compact", "orchestrator"]);
    assert!(response.contains("scheduled"), "{response}");
    horch_e2e::harness::wait_for("orchestrator compaction", || {
        ledger
            .get("r-orchestrator")
            .ok()?
            .history
            .iter()
            .any(|e| e.event == "compacted")
            .then_some(())
    });
    let calls = h.calls_of("herdr");
    let lines: Vec<_> = calls
        .iter()
        .filter_map(|c| c["argv"].as_array())
        .filter(|a| {
            a.first().and_then(|v| v.as_str()) == Some("agent")
                && a.get(1).and_then(|v| v.as_str()) == Some("prompt")
        })
        .filter_map(|a| a.get(3).and_then(|v| v.as_str()))
        .collect();
    // Sanctioned actual compact argument (#57): keep the real run identity and
    // decisions-log path; roster and ownership come from the task ledger.
    let compact_lines: Vec<_> = lines
        .iter()
        .filter(|l| l.starts_with("/compact "))
        .collect();
    assert_eq!(compact_lines.len(), 1, "{lines:?}");
    for required in [
        "your actual run slug",
        "ai_docs/plans/<run-slug>/decisions.md",
        "Use the actual slug and path from your run, not placeholder text",
        "rebuild the roster and ownership from horch ledger status instead of retaining them",
    ] {
        assert!(
            compact_lines[0].contains(required),
            "missing {required}: {}",
            compact_lines[0]
        );
    }
    let cards: Vec<_> = lines
        .iter()
        .filter(|l| l.contains("Command card:"))
        .collect();
    assert_eq!(cards.len(), 1, "{lines:?}");
    assert!(!cards[0].contains('\n'));
    for command in [
        "horch ledger status",
        "horch ledger plan-workers",
        "horch spawn",
        "horch tell",
        "horch inbox",
        "horch ledger events",
        "horch context --over",
        "horch compact",
        "horch note --fact",
    ] {
        assert!(cards[0].contains(command), "missing {command}");
    }
}

#[test]
fn g58_claude_workers_share_read_only_preapprovals() {
    let r = Roster::builtin().unwrap();
    let expected = &r.require("sonnet").unwrap().allowed_tools;
    assert!(expected.iter().any(|p| p == "Bash(git status *)"));
    let mut count = 0;
    for name in r.names() {
        let t = r.require(name).unwrap();
        if t.agent.as_str() == "claude" && t.base.as_deref() == Some("fleet-worker") {
            assert_eq!(&t.allowed_tools, expected, "{name}");
            count += 1;
        }
    }
    assert_eq!(count, 62);
    for pattern in expected {
        assert!(
            matches!(pattern.as_str(), "Read" | "Grep" | "Glob") || pattern.starts_with("Bash(")
        );
        for forbidden in [
            " push",
            " install",
            " commit",
            " checkout",
            " reset",
            " clean",
            " delete",
            " remove",
            " apply",
            "&&",
            ";",
            "|",
            ">",
            "<",
            "`",
        ] {
            assert!(!pattern.contains(forbidden), "{pattern}");
        }
    }
}

fn detached_drop_is_refused(keep_branch: bool) {
    let (h, w) = ledger_world(if keep_branch {
        "g69-det-keep"
    } else {
        "g69-det-drop"
    });
    let made = worker_ok(&h, &w, &["ledger", "worktree", "new", "detached"]);
    let path = tree_path(&made);
    let recorded_branch = git(&h, &path, &["branch", "--show-current"]);
    let recorded_commit = git(&h, &path, &["rev-parse", "HEAD"]);
    git(&h, &path, &["checkout", "--detach"]);
    std::fs::write(path.join("detached.txt"), "unique detached work\n").unwrap();
    git(&h, &path, &["add", "detached.txt"]);
    git(&h, &path, &["commit", "-q", "-m", "detached unique work"]);
    let live_commit = git(&h, &path, &["rev-parse", "HEAD"]);
    assert_ne!(live_commit, recorded_commit);
    assert!(git(
        &h,
        &h.project,
        &["for-each-ref", "--contains", &live_commit]
    )
    .is_empty());
    let mut args = vec![
        "ledger",
        "worktree",
        "drop",
        "T1-detached",
        "--reason",
        "stop review",
    ];
    if keep_branch {
        args.push("--keep-branch");
    }
    let refused = as_worker(&h, &w, &args);
    assert!(
        !refused.status.success(),
        "detached drop must refuse (keep={keep_branch}): {refused:?}"
    );
    assert!(path.join("detached.txt").is_file());
    assert_eq!(git(&h, &path, &["rev-parse", "HEAD"]), live_commit);
    assert_eq!(
        git(&h, &h.project, &["rev-parse", &recorded_branch]),
        recorded_commit
    );
    let board: serde_json::Value =
        serde_json::from_slice(&h.run(&["ledger", "status", "--json"]).stdout).unwrap();
    assert_eq!(board["worktrees"][0]["status"], "open");
    assert!(board["worktrees"][0]["reason"].is_null());
}

#[test]
fn g69_drop_refuses_detached_unique_commits() {
    detached_drop_is_refused(false);
}
#[test]
fn g69_drop_keep_branch_refuses_detached_unique_commits() {
    detached_drop_is_refused(true);
}

#[test]
fn g69_drop_refuses_changed_checked_out_branch() {
    let (h, w) = ledger_world("g69-mismatch");
    let made = worker_ok(&h, &w, &["ledger", "worktree", "new", "changed"]);
    let path = tree_path(&made);
    git(&h, &path, &["checkout", "-b", "actual-work"]);
    std::fs::write(path.join("actual.txt"), "actual branch work\n").unwrap();
    git(&h, &path, &["add", "actual.txt"]);
    git(&h, &path, &["commit", "-q", "-m", "actual branch"]);
    let live = git(&h, &path, &["rev-parse", "HEAD"]);
    for keep in [false, true] {
        let mut args = vec![
            "ledger",
            "worktree",
            "drop",
            "T1-changed",
            "--reason",
            "stop",
        ];
        if keep {
            args.push("--keep-branch");
        }
        let o = as_worker(&h, &w, &args);
        assert!(!o.status.success(), "mismatched branch must refuse: {o:?}");
        assert_eq!(git(&h, &path, &["rev-parse", "HEAD"]), live);
        assert!(path.join("actual.txt").is_file());
    }
}
