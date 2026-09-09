//! Proof that moving the briefings into `teammates/` changed no text.
//!
//! The files in `tests/golden/` were captured from the previous implementation,
//! where every prompt was a Rust `const` or `format!`. Each is compared byte for
//! byte against what the roster renders now. If a `.md` file is edited, these
//! fail - which is the point: prompt text is a deliberate change, not a
//! side effect of touching code.
//!
//! The orchestrator briefing has three sanctioned differences, all spelled out
//! in `the_orchestrator_briefing_differs_only_where_sanctioned`: a
//! hand-maintained list of five tiers became the `{roster}` placeholder, the
//! fleet no longer pre-spawns four idle workers for it to inherit, and it is
//! told it is the only Fable and must write for Opus/Codex-Sol readers.

use horch_core::prompts;
use horch_core::teammates::Roster;

fn golden(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Compare against the compiled-in roster, not a runtime directory: this must
/// pass on a machine that has never seen this repo.
fn roster() -> Roster {
    Roster::builtin().expect("built-in roster parses")
}

#[test]
fn every_worker_briefing_is_unchanged() {
    let r = roster();
    let mut checked = 0;
    // No `fable` here: the generic Fable worker was removed when Fable became
    // the orchestrator's reserved model. Its goldens went with it.
    for name in ["sonnet", "opus", "codex-sol", "codex-terra", "smoke"] {
        let t = r.require(name).unwrap();
        for (label, task, resume) in [
            ("task", "Investigate the failing test in auth.go", false),
            ("idle", "", false),
            ("resume", "Continue where you left off", true),
        ] {
            let got = prompts::worker_prompt(&r, t, "r-1", task, resume).unwrap();
            assert_eq!(
                got,
                golden(&format!("worker-{name}-{label}")),
                "worker-{name}-{label} drifted"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 15);
}

#[test]
fn the_orchestration_recipe_briefings_are_unchanged() {
    let r = roster();
    assert_eq!(
        prompts::agent_prompt(&r, r.require("orchestration-orchestrator").unwrap(), "orchestrator")
            .unwrap(),
        golden("orchestration-orchestrator")
    );
    assert_eq!(
        prompts::agent_prompt(&r, r.require("orchestration-worker").unwrap(), "sonnet-1").unwrap(),
        golden("orchestration-worker")
    );
}

/// The orchestrator briefing differs in exactly three places, and this pins them.
/// Anything else that drifts fails here rather than in a live pane.
#[test]
fn the_orchestrator_briefing_differs_only_where_sanctioned() {
    let r = roster();
    let got = prompts::agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
    let was = golden("fleet-orchestrator");

    let old_block = "Pick the tier by complexity:\n  sonnet       clear, easy, \
                     well-specified junior/grunt work\n  opus         sophisticated but \
                     guided work - direction decided, judgment needed\n  fable        \
                     planning, research, design (it reasons on Fable and sends its\n               \
                     subagents to Haiku for file-digging legwork)\n  codex-sol    complex \
                     implementation work\n  codex-terra  grunt work, Codex flavor";
    let new_block = format!(
        "Pick the teammate whose description fits the work:\n{}",
        r.roster_lines()
    );
    // Second sanctioned change: `horch fleet` starts one orchestrator pane and
    // nothing else, so the briefing can no longer promise a 2x2 grid of idle
    // workers that were never spawned.
    let old_fleet = "in your current directory. You start with four idle workers in a 2x2 grid\n\
                     beside you - sonnet-1, sonnet-2 (Claude Sonnet), opus-1 (Claude Opus),\n\
                     codex-sol-1 (Codex, gpt-5.6-sol) - and you grow or shrink the fleet yourself\n\
                     as the work demands.";
    let new_fleet = "in your current directory. You start ALONE - there are no workers yet. Break\n\
                     the work down, then spawn exactly the workers each piece needs and shut them\n\
                     down as they finish. Never spawn a worker before you know what it is for.";

    // Third: Fable is reserved for the orchestrator, so it is told so and told
    // how to write for the models its workers actually run on. Inserted whole,
    // immediately before the lifecycle section.
    let lifecycle = "== Worker lifecycle ==";
    let only_fable = "== You are the only Fable ==\n\
        You are the only Fable session in this fleet, and horch spawn refuses to\n\
        start another. Every worker reads your instructions on Opus or Codex Sol at\n\
        best, often on something cheaper. Write every task for that reader:\n\
        - State the goal and what \"done\" looks like, explicitly. Do not leave the\n\
        \x20 acceptance criteria to be inferred.\n\
        - Name the files, functions and commands involved. \"The auth layer\" is a\n\
        \x20 guess you are asking the worker to make.\n\
        - Spell out the steps and the check for each one. Ordering you find obvious\n\
        \x20 is not obvious to the worker.\n\
        - Say what is out of scope. Unstated boundaries get crossed.\n\
        A brief you would find slightly over-specified is about right for them.\n\
        When a piece of work needs Fable-level reasoning - a design with real\n\
        tradeoffs, a plan across many moving parts, a judgement call - that reasoning\n\
        is yours. Do it here, write the result to a file, and hand the execution to\n\
        opus. Never delegate the thinking itself downward and hope.\n\n";
    assert_eq!(was.matches(lifecycle).count(), 1);

    let expected = was
        .replace(old_block, &new_block)
        .replace(old_fleet, new_fleet)
        .replace(lifecycle, &format!("{only_fable}{lifecycle}"));
    assert_eq!(
        expected, got,
        "the orchestrator briefing changed outside the three sanctioned blocks"
    );
}

/// Execpolicy rules render exactly as they did, so an existing
/// `~/.codex/execpolicy` is not duplicated on upgrade.
#[test]
fn execpolicy_blocks_are_unchanged() {
    let r = roster();
    for rule in r.exec_rules() {
        let name = format!("execpolicy-{}", rule.justification.replace(' ', "-"));
        assert_eq!(prompts::codex_rule_block(rule), golden(&name), "{name} drifted");
    }
    assert_eq!(r.exec_rules().len(), 3);
}
