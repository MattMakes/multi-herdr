# V2 roster-offer: offer domain teammates only where the project needs them

Unit slug: `roster-offer`. Branch: `ds/roster-offer`.

## GOAL

A teammate with `offer_when: ["*.uproject"]` (globs, any match) is listed in
the orchestrator's roster briefing only when the fleet's project directory
contains a matching file (top level, or 1 level down). `horch spawn <it>`
still works anywhere. `horch teammates` shows the field. `horch doctor`
checks for `xcodebuild` (and its first-launch state) when any offered
teammate needs it (`requires: [xcode]` or derive from a field you add).

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`,
  `ai_docs/reports/unreal-engine-wave.md` "Code changes" item 3,
  `ai_docs/reports/swift-fleet-skills-2026-10.md` §7.1.
- Planned uses: Unreal teammates `offer_when: ["*.uproject"]`; Swift and
  Apple teammates `offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]`.
- Code: the orchestrator prompt renders `{roster}` via `Roster::roster_lines`
  (`crates/horch-core/src/roster/`, `prompts.rs`); `horch fleet`
  (`crates/horch/src/cmd/recipes.rs`) knows the project dir;
  `crates/horch-core/tests/golden_prompts.rs` builds the roster dynamically.
  The roster filter must be a pure function of (roster, project file facts);
  the CLI gathers the facts.
- Must not change the orchestrator golden for a project without matches: a
  teammate without `offer_when` is always offered.

## FILES

own: `crates/horch-core/src/roster/**` (the field and the filter),
`crates/horch-core/src/prompts.rs` (pass the filter), `crates/horch/src/cmd/recipes.rs`
and `cmd/doctor.rs` (facts and the check), tests next to the code,
`teammates/_template.md` (document the field), `docs/recipes/add-teammate.md`,
`ai_docs/reports/domain-skills/roster-offer.md`.

## STEPS

0. Create the worktree.
1. Field + pure filter + tests (match at top level, 1 level down, none).
2. Wire into the orchestrator roster; test that the fleet orchestrator
   briefing hides a non-matching teammate and shows a matching one.
3. `horch doctor` xcode check with a fake `xcodebuild` in tests.
4. Gate. Commits per step. Report. Follow the merge protocol.
