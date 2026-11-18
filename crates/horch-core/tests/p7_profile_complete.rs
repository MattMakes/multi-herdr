//! A worker profile is complete (FR-6; ticket 20 of fleetd-p7). Each field
//! that FR-6 names - CLI, model, skills, MCP servers, sandbox, writable
//! paths, context budget, timeouts and restart policy - resolves to a value
//! from the file, its `base`, or a default of the design. `horch teammates
//! --check` refuses a worker profile with a field that has no value and no
//! default, with the file and the field (`docs/tickets/fleetd-p7/MAP.md`,
//! Q7). The exempt teammates of fleetd-p2 Q8 are not worker profiles.
//!
//! Each test overlays 1 teammate file from a temp folder on the built-in
//! roster. No test starts an agent CLI.

use horch_core::roster::Roster;

/// The file of a Claude worker profile `name`, complete apart from what
/// `extra` and `without` change.
fn claude_file(name: &str, without: &[&str], extra: &str) -> String {
    let lines = [
        ("agent", "agent: claude\n".to_string()),
        ("model", "model: opus\n".to_string()),
        ("fallbacks", "fallbacks: [codex-sol]\n".to_string()),
        ("compact_window", "compact_window: 256000\n".to_string()),
        (
            "sandbox",
            "sandbox:\n  network:\n    allowedDomains: [\"*\"]\n".to_string(),
        ),
    ];
    let mut text = format!(
        "---\nname: {name}\nbrief_description: test worker\nbase: fleet-worker\n\
         compact_at: 200000\ndisallowed_tools: [Agent]\n"
    );
    for (field, line) in lines {
        if !without.contains(&field) {
            text.push_str(&line);
        }
    }
    text.push_str(extra);
    text.push_str("---\nbody\n");
    text
}

/// The `horch teammates --check` lines of the built-in roster with the
/// file `name.md` of `text` laid over it, that name `name`.
fn check_lines(name: &str, text: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(format!("{name}.md")), text).unwrap();
    let mut roster = Roster::builtin().unwrap();
    roster.overlay(dir.path()).unwrap();
    roster
        .check()
        .into_iter()
        .filter(|line| line.contains(name))
        .collect()
}

/// The lines of `lines` that refuse an incomplete profile for `field`,
/// with the file `name.md`.
fn incomplete<'a>(lines: &'a [String], name: &str, field: &str) -> Vec<&'a String> {
    let prefix = format!("{name}.md: ");
    let marker = format!("incomplete worker profile: {field} has no value");
    lines
        .iter()
        .filter(|l| l.starts_with(&prefix) && l.contains(&marker))
        .collect()
}

fn no_incomplete_line(lines: &[String]) -> bool {
    !lines
        .iter()
        .any(|l| l.contains("incomplete worker profile"))
}

#[test]
fn a_complete_worker_profile_passes_the_check() {
    let lines = check_lines("p7-whole", &claude_file("p7-whole", &[], ""));
    assert!(lines.is_empty(), "{lines:#?}");
}

#[test]
fn worker_profile_without_cli_is_refused_with_the_file_and_the_field() {
    let lines = check_lines("p7-nocli", &claude_file("p7-nocli", &["agent"], ""));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("p7-nocli.md") && l.contains("missing field `agent`")),
        "{lines:#?}"
    );
}

#[test]
fn worker_profile_without_model_is_refused_with_the_field() {
    let lines = check_lines("p7-nomodel", &claude_file("p7-nomodel", &["model"], ""));
    assert_eq!(
        incomplete(&lines, "p7-nomodel", "model").len(),
        1,
        "{lines:#?}"
    );
}

#[test]
fn worker_profile_without_sandbox_is_refused_with_the_field() {
    let lines = check_lines("p7-nobox", &claude_file("p7-nobox", &["sandbox"], ""));
    assert_eq!(
        incomplete(&lines, "p7-nobox", "sandbox").len(),
        1,
        "{lines:#?}"
    );
}

#[test]
fn worker_profile_without_writable_paths_is_refused_with_the_field() {
    // The Claude adapter renders the writable paths of the design
    // ({task_dir}, {worktree}) into the sandbox. With no sandbox they
    // have no value.
    let lines = check_lines("p7-nowrite", &claude_file("p7-nowrite", &["sandbox"], ""));
    assert_eq!(
        incomplete(&lines, "p7-nowrite", "writable_paths").len(),
        1,
        "{lines:#?}"
    );
}

#[test]
fn worker_profile_without_context_budget_is_refused_with_the_field() {
    // No compact_window, and no key of context-windows.md for this model.
    let text = claude_file(
        "p7-noctx",
        &["compact_window", "model"],
        "model: p7-model\n",
    );
    let lines = check_lines("p7-noctx", &text);
    assert_eq!(
        incomplete(&lines, "p7-noctx", "context_budget").len(),
        1,
        "{lines:#?}"
    );
}

#[test]
fn context_budget_resolves_from_context_windows_md() {
    // claude/opus has a key in context-windows.md: the design default.
    let text = claude_file("p7-ctxbase", &["compact_window"], "");
    let lines = check_lines("p7-ctxbase", &text);
    assert!(no_incomplete_line(&lines), "{lines:#?}");
}

#[test]
fn context_budget_of_a_cli_with_no_window_setting_is_the_wrapper_limit() {
    // OpenCode has no window setting: the wrapper ends the attempt with a
    // handoff at 256k, a default of the design.
    let text = "---\nname: p7-oc\nbrief_description: test worker\nbase: fleet-worker\n\
                agent: opencode\nmodel: opencode/p7-model\nfallbacks: [opus]\n---\nbody\n";
    let lines = check_lines("p7-oc", text);
    assert!(no_incomplete_line(&lines), "{lines:#?}");
}

#[test]
fn skills_and_mcp_servers_may_be_empty_lists() {
    let text = claude_file("p7-empty", &[], "skills: []\nmcp_servers: {}\n");
    let lines = check_lines("p7-empty", &text);
    assert!(lines.is_empty(), "{lines:#?}");
}

#[test]
fn a_skill_that_does_not_resolve_is_refused_with_the_field() {
    let text = claude_file("p7-skill", &[], "skills: [p7-no-such-skill]\n");
    let lines = check_lines("p7-skill", &text);
    assert!(
        lines.iter().any(|l| l.contains("p7-no-such-skill")),
        "{lines:#?}"
    );
}

#[test]
fn an_mcp_config_file_that_does_not_resolve_is_refused_with_the_field() {
    let text = claude_file("p7-mcp", &[], "mcp_config_files: [/p7/no/such/mcp.json]\n");
    let lines = check_lines("p7-mcp", &text);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("mcp_config_files") && l.contains("/p7/no/such/mcp.json")),
        "{lines:#?}"
    );
}

#[test]
fn timeouts_and_restart_policy_resolve_to_the_design_defaults() {
    let roster = {
        let dir = tempfile::tempdir().unwrap();
        let text = claude_file("p7-dflt", &[], "");
        std::fs::write(dir.path().join("p7-dflt.md"), text).unwrap();
        let mut r = Roster::builtin().unwrap();
        r.overlay(dir.path()).unwrap();
        r
    };
    let profile = roster.get("p7-dflt").unwrap().profile();
    let defaults = horch_core::fleetd::worker_profiles::domain::profile::Profile::default();
    assert_eq!(profile.liveness, defaults.liveness);
    assert_eq!(profile.retry, defaults.retry);
    assert!(profile.problems().is_empty());
}

#[test]
fn a_zero_timeout_is_refused_with_the_field() {
    let text = claude_file("p7-zero", &[], "liveness:\n  start_timeout_s: 0\n");
    let lines = check_lines("p7-zero", &text);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("p7-zero.md: ") && l.contains("liveness.start_timeout_s")),
        "{lines:#?}"
    );
}

#[test]
fn a_zero_restart_count_is_refused_with_the_field() {
    let text = claude_file("p7-retry", &[], "retry:\n  max_attempts: 0\n");
    let lines = check_lines("p7-retry", &text);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("p7-retry.md: ") && l.contains("retry.max_attempts")),
        "{lines:#?}"
    );
}

#[test]
fn an_exempt_teammate_is_not_a_worker_profile() {
    // agent: none runs no agent, so it has no worker profile to complete.
    let text = "---\nname: p7-none\nbrief_description: test\nagent: none\n---\nbody\n";
    let lines = check_lines("p7-none", text);
    assert!(no_incomplete_line(&lines), "{lines:#?}");
}

#[test]
fn repo_roster_has_only_complete_worker_profiles() {
    let roster = Roster::builtin().unwrap();
    let incomplete: Vec<String> = roster
        .check()
        .into_iter()
        .filter(|l| l.contains("incomplete worker profile"))
        .collect();
    assert!(incomplete.is_empty(), "{incomplete:#?}");
}
