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
//!
//! The worker briefing has one, in
//! `every_worker_briefing_differs_only_where_sanctioned`: the orchestrator is
//! described to workers as a separate *agent* session rather than a separate
//! Claude one, because the pane it sits in is no longer always Claude.
//!
//! Both base briefings also gained the Simplified Technical English section
//! (`ste_section`), inserted whole and pinned in those same two tests.

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

/// The `== Message style: Simplified Technical English ==` section that both
/// base briefings gained, as it renders: the rules are word for word the same
/// in `fleet-worker.md` and `fleet-orchestrator.md`, and only the lead-in (who
/// writes which messages) and the one example differ.
fn ste_section(lead: &[&str], example: &str) -> String {
    let rules = [
        "- Write one instruction or one fact in each sentence. A procedural sentence",
        "  has at most 20 words. A descriptive sentence has at most 25 words.",
        "- Use the active voice and the present tense. Name the actor.",
        "- Use one word for one thing. Do not use synonyms for variety.",
        "- Do not use idioms, metaphors, or hedges such as \"it seems\", \"sort of\",",
        "  \"basically\".",
        "- Write paths, commands, flags, and identifiers exactly as they are. Put one",
        "  per sentence when possible.",
        "- Use a list for parallel items, one item per line. Do not nest lists.",
        "- Start a report with the role tag and one keyword: `ready`, `DONE:`,",
        "  `BLOCKED:`, `NOTE:`, or `QUESTION:`. Then write one sentence with the",
        "  outcome. Then write the details.",
        "- Write numbers as digits and state units. Give exact counts when you know",
        "  them.",
        "- Put a warning before the action it applies to.",
    ];
    format!(
        "== Message style: Simplified Technical English ==\n{}\n{}\nExample:\n  {example}\n\n",
        lead.join("\n"),
        rules.join("\n"),
    )
}

/// The worker briefing differs in exactly two places, and this pins them. The
/// first: the orchestrator is no longer described to workers as a Claude session.
///
/// `horch fleet codex` puts a Codex orchestrator in that pane, and OpenCode and
/// pi are coming, so naming any one CLI there is a briefing that is wrong for
/// most of the fleets that read it. A worker only needs to know the orchestrator
/// is a SEPARATE session whose typed lines are instructions - which agent is
/// behind it changes nothing it does. The fixed `orchestration` recipe still
/// says "Claude ... running Fable" in `orchestration-worker.md`, because there
/// the orchestrator really is pinned to one agent and one model.
#[test]
fn every_worker_briefing_differs_only_where_sanctioned() {
    let r = roster();
    let mut checked = 0;
    // Second sanctioned change: the Simplified Technical English rule, inserted
    // whole at the end of the "Communication and lifecycle" block. Every worker
    // in these goldens has the role "r-1", so that is the tag the example shows.
    let lifecycle_end = "gotchas, current state.\n\n";
    let ste = ste_section(
        &[
            "Write every `horch tell` message, every `horch done` message, and every",
            "`[r-1]` line in Simplified Technical English (STE, ASD-STE100 style).",
        ],
        "[r-1] DONE: The report is at ai_docs/reports/x.md. I changed 2 files. \
         Tests pass: 14 of 14. Nothing is uncommitted.",
    );
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
            let was = golden(&format!("worker-{name}-{label}"));
            let old_agent = "orchestrator (a separate Claude session) runs in another pane";
            let new_agent = "orchestrator (a separate agent session) runs in another pane";
            assert_eq!(
                was.matches(old_agent).count(),
                1,
                "worker-{name}-{label} should name the orchestrator's agent exactly once"
            );
            assert_eq!(
                was.matches(lifecycle_end).count(),
                1,
                "worker-{name}-{label} should end its lifecycle block exactly once"
            );
            assert_eq!(
                was.replace(old_agent, new_agent)
                    .replace(lifecycle_end, &format!("{lifecycle_end}{ste}")),
                got,
                "worker-{name}-{label} drifted outside the two sanctioned blocks"
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
        prompts::agent_prompt(
            &r,
            r.require("orchestration-orchestrator").unwrap(),
            "orchestrator"
        )
        .unwrap(),
        golden("orchestration-orchestrator")
    );
    assert_eq!(
        prompts::agent_prompt(&r, r.require("orchestration-worker").unwrap(), "sonnet-1").unwrap(),
        golden("orchestration-worker")
    );
}

/// The orchestrator briefing differs in exactly six places, and this pins them.
/// Anything else that drifts fails here rather than in a live pane.
#[test]
fn the_orchestrator_briefing_differs_only_where_sanctioned() {
    let r = roster();
    let got =
        prompts::agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
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
    let new_fleet =
        "in your current directory. You start ALONE - there are no workers yet. Break\n\
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

    let old_spawning = "  horch spawn <tier> \"<task>\"                            new session\n  horch spawn --resume <session-or-record-id> \"<task>\"   resume old session\n";
    let new_spawning = "  horch spawn <tier> [--phase <phase>] \"<task>\"            new session\n\
        \x20 horch spawn --resume <session-or-record-id> [--phase <phase>] \"<task>\"\n\
        \x20                                                      resume old session\n\n\
        Choose research, plan, implementation, or validation with --phase when the\n\
        assignment differs from the teammate's default. Each phase exposes a small\n\
        portable skill catalog; workers read only matching skill bodies as needed.\n\
        Resume keeps the recorded phase unless --phase overrides it. At phase handoff,\n\
        pass the findings, plan, changed files, and validation evidence by file path;\n\
        start or resume a worker with the next phase instead of asking it to preload\n\
        every phase's instructions.\n\n";
    assert_eq!(was.matches(old_spawning).count(), 1);

    // Fifth: the Simplified Technical English rule, inserted whole between the
    // channel section, which introduces `horch assign` and `horch tell`, and
    // the ledger section.
    let ledger = "== Session ledger: resume vs fresh ==";
    let ste = ste_section(
        &[
            "Write every `horch assign` message and every `horch tell` message in",
            "Simplified Technical English (STE, ASD-STE100 style). Workers write their",
            "`horch done` messages and their `[<role>]` lines in STE too.",
        ],
        "horch assign sonnet-1 \"Read and follow ai_docs/plans/x.md. Edit only the \
         files that the plan names. Send DONE: when the tests pass.\"",
    );
    assert_eq!(was.matches(ledger).count(), 1);

    // Sixth: the workers-only rule, inserted as its own bullet directly after
    // the sentence that tells the orchestrator to spawn workers to do the work.
    // The rest of that paragraph starts again at the left margin.
    let spawn_sentence = "spawn workers to do the work, you just breakdown and organize/plan the\n\
                          tasks. ";
    let workers_only = "spawn workers to do the work, you just breakdown and organize/plan the\n\
                        tasks.\n\
                        - Use your fleet workers only. Do not use subagents, the Agent tool,\n\
                        \x20 background tasks, or any in-session delegation. Every piece of delegated\n\
                        \x20 work goes through `horch spawn` or `horch assign`, so it is visible in the\n\
                        \x20 ledger and the grid.\n";
    assert_eq!(was.matches(spawn_sentence).count(), 1);

    let expected = was
        .replace(old_block, &new_block)
        .replace(old_fleet, new_fleet)
        .replace(old_spawning, new_spawning)
        .replace(lifecycle, &format!("{only_fable}{lifecycle}"))
        .replace(ledger, &format!("{ste}{ledger}"))
        .replace(spawn_sentence, workers_only);
    assert_eq!(
        expected, got,
        "the orchestrator briefing changed outside the six sanctioned blocks"
    );
}

/// Execpolicy rules render exactly as they did, so an existing
/// `~/.codex/execpolicy` is not duplicated on upgrade.
#[test]
fn execpolicy_blocks_are_unchanged() {
    let r = roster();
    for rule in r.exec_rules() {
        let name = format!("execpolicy-{}", rule.justification.replace(' ', "-"));
        assert_eq!(
            prompts::codex_rule_block(rule),
            golden(&name),
            "{name} drifted"
        );
    }
    assert_eq!(r.exec_rules().len(), 3);
}

/// The Codex flavor shares the whole orchestrator briefing and differs only in
/// the block naming the tier it is the only session of. This is not a golden -
/// there is no prior behaviour to capture - so it pins the relationship
/// instead: same briefing, one substituted paragraph, fully rendered.
#[test]
fn the_codex_orchestrator_differs_from_the_claude_one_only_in_its_tier_block() {
    let r = roster();
    let claude =
        prompts::agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
    let codex = prompts::agent_prompt(&r, r.require("orchestrator-codex").unwrap(), "orchestrator")
        .unwrap();

    assert!(codex.contains("== You are the only Astra =="), "{codex}");
    assert!(!codex.contains("== You are the only Fable =="), "{codex}");
    // No placeholder may survive into a live pane - not {roster}, not {persona}.
    assert!(
        !codex.contains('{'),
        "unsubstituted placeholder in:\n{codex}"
    );
    assert!(
        codex.contains("codex-terra"),
        "the roster must reach it: {codex}"
    );

    // Everything outside the tier block is byte-identical to the Claude flavor.
    let strip = |text: &str| {
        let start = text.find("== You are the only ").expect("a tier block");
        let end = text
            .find("== Worker lifecycle ==")
            .expect("the lifecycle section");
        format!("{}{}", &text[..start], &text[end..])
    };
    assert_eq!(strip(&claude), strip(&codex));
}
