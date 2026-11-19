//! Ticket 27 of fleetd-p6: the `idea-lead` persona, the interactive
//! sub-orchestrator of 1 feature (`docs/specs/fleetd/design.md`, Architecture
//! and roles, "Sub-orchestrator"; Ideas; Worker profiles and adapters, "Exec
//! or interactive"). Its frontmatter and its briefing are static text, so
//! these tests pin them: the roster loads it, it is interactive, its sandbox
//! denies the operator path (SR-8, FM-32), and its briefing names each phase.

use std::path::Path;

use horch_core::execution::SessionMode;
use horch_core::fleetd::worker_profiles::domain::profile::Mode;
use horch_core::harness::launch;
use horch_core::harness::{HarnessKind, LaunchEnv, Session};
use horch_core::prompts;
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::Value;

/// The fleet root beside the default ledger `~/.horch/ledger.sqlite3`
/// (`runtime::paths`, `fleet_root`).
const FLEET: &str = "~/.horch/fleet";

fn roster() -> Roster {
    Roster::builtin().expect("built-in roster parses")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

/// A launch environment with a temp home and stub Linux sandbox tools, so
/// the builder runs on every host.
fn launch_env(home: &Path) -> LaunchEnv {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["bwrap", "socat"] {
        let path = bin.join(tool);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    let env = MapEnv::new(home)
        .with("HOME", &home.to_string_lossy())
        .with("PATH", &bin.to_string_lossy())
        .with(
            "HERDR_SOCKET_PATH",
            &home.join("herdr.sock").to_string_lossy(),
        )
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &home.join("no-managed-settings.json").to_string_lossy(),
        );
    LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap())
}

/// The `--settings` that the launch of `t` gives Claude Code.
fn launched_settings(env: &LaunchEnv, t: &Teammate) -> Value {
    let cmd =
        launch::command_with_skills_in(env, t, Session::Unmanaged, "PROMPT", None, None).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let i = args
        .iter()
        .position(|a| a == "--settings")
        .expect("a --settings argument");
    serde_json::from_str(&args[i + 1]).unwrap()
}

#[test]
fn idea_lead_loads_from_the_roster() {
    let r = roster();
    let t = r.require("idea-lead").expect("idea-lead is in the roster");
    assert_eq!(t.agent, HarnessKind::Claude);
    assert_eq!(t.base.as_deref(), Some("fleet-idea-lead"));
    assert_eq!(t.model.as_deref(), Some("opus"));
    assert_eq!(t.effort.as_deref(), Some("medium"));
    // A fallback launches without the sandbox block, so it names none.
    assert!(t.fallbacks.is_empty(), "{:?}", t.fallbacks);
    assert!(t.no_fallback_reason.is_some());
    assert!(
        r.base("fleet-idea-lead").is_some(),
        "the base _base/fleet-idea-lead.md is compiled in"
    );
    let problems: Vec<String> = r
        .check()
        .into_iter()
        .filter(|p| p.contains("idea-lead"))
        .collect();
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn idea_lead_is_interactive() {
    let t = roster().require("idea-lead").unwrap().clone();
    let profile = t.profile();
    assert_eq!(profile.mode, Mode::Interactive);
    assert!(
        t.mode.is_some(),
        "the file states the mode, not the default"
    );
    assert_eq!(profile.fallback(), None);
}

#[test]
fn idea_lead_sandbox_denies_the_operator_path() {
    let r = roster();
    let t = r.require("idea-lead").unwrap();
    let token = format!("{FLEET}/run/operator.token");
    let run = format!("{FLEET}/run");
    let ideas = format!("{FLEET}/ideas");

    let own = Value::Object(t.sandbox.clone().expect("idea-lead has a sandbox"));
    let fs = &own["filesystem"];
    assert_eq!(
        strings(&fs["allowWrite"]),
        [ideas.clone()],
        "its idea folders only"
    );
    assert!(
        strings(&fs["denyRead"]).contains(&token),
        "no read of the token"
    );
    assert!(
        strings(&fs["denyWrite"]).contains(&run),
        "no write into run/: the request folder and the channel socket"
    );

    // FM-32: the profile denies the approve command and the write of
    // approval.json.
    assert!(
        t.disallowed_tools
            .iter()
            .any(|d| d == "Bash(horch idea approve*)"),
        "{:?}",
        t.disallowed_tools
    );
    assert!(
        t.disallowed_tools
            .iter()
            .any(|d| d == &format!("Edit({ideas}/**/approval.json)")),
        "{:?}",
        t.disallowed_tools
    );

    // The launch keeps each denial, and opens no socket under run/.
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let settings = launched_settings(&env, t);
    let s = &settings["sandbox"];
    assert_eq!(s["enabled"], true);
    assert!(strings(&s["filesystem"]["denyRead"]).contains(&token));
    assert!(strings(&s["filesystem"]["denyWrite"]).contains(&run));
    let home_run = home.path().join(".horch/fleet/run");
    for socket in strings(&s["network"]["allowUnixSockets"]) {
        let p = Path::new(&socket);
        assert!(
            !socket.starts_with(&run) && !p.starts_with(&home_run) && !home_run.starts_with(p),
            "the channel socket is reachable through {socket}"
        );
    }
}

#[test]
fn idea_lead_briefing_names_each_phase() {
    let r = roster();
    let t = r.require("idea-lead").unwrap();
    let p = prompts::worker_prompt(
        &r,
        t,
        "idea-0012",
        "Phase: clarifying",
        &SessionMode::Fresh(None),
    )
    .unwrap();
    for phase in ["clarifying", "specifying", "ticketing", "supervising"] {
        assert!(p.contains(&format!("== Phase: {phase} ==")), "{phase}");
    }
    for needle in [
        // clarifying
        "horch idea note",
        "1 question at a time",
        "assumptions.md",
        "feature: <name>",
        "[open]",
        // specifying
        "spec.md",
        "requirements.yaml",
        "A<n>",
        "Q<n>",
        // ticketing
        "plan.yaml",
        "docs/specs/ticket-format.md",
        "horch idea show <id> --in-flight",
        "feature id F-NNNN",
        // supervising
        "horch idea note <id> --question",
        "replan-<task key>.yaml",
        "HIGH",
        "--dismiss",
        // each phase
        "Answer the open questions of the feature before you go on with your phase.",
        "Never run horch idea approve",
        "approval.json",
    ] {
        assert!(p.contains(needle), "the briefing does not name {needle:?}");
    }
    // Project facts reach each sub-orchestrator as they reach each worker.
    let worker = r.require("opus").unwrap();
    let w = prompts::worker_prompt(&r, worker, "w-1", "x", &SessionMode::Fresh(None)).unwrap();
    assert!(w.contains("Project facts:"));
    assert!(p.contains("Project facts:"));
    assert!(p.contains("horch note --fact"));
}
