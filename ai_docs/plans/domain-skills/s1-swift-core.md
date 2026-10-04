# S1 swift-core: core Swift and SwiftUI skills

Unit slug: `swift-core`. Branch: `ds/swift-core`.

## GOAL

Bundle the report's §4.1 skills: swiftui-pro, swift-concurrency-pro,
swiftdata-pro, swift-testing-pro, swift-format-style, swiftui-liquid-glass,
observability, each with its report edits applied.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, then
  `ai_docs/reports/swift-fleet-skills-2026-10.md` §4 (your rows), §5 (house
  style: every adaptation must agree with the "fleet answer" column) and
  §7.3 (provenance).
- Sources: `/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/<owner>_<repo>/`; pins in `/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/PINS.txt`.
- "Vendor" means verbatim (`vendored: true`, budget-exempt). "Adapt" means
  our curated copy: apply exactly the report's edits plus the fleet rules
  (no subagents, `context: fork` removed, no "ask the user" — a message to
  the orchestrator instead, no `${CLAUDE_*}` variables, links to other
  upstream repos replaced by the bundled skill name). An adapted skill is
  `vendored: true` only if you leave its references verbatim and it is over
  budget; otherwise it must fit the 12 KB / 160 KB budget.
- Where the report says "checked" a fix is required. Where it says
  "verify", verify against Apple's documentation (web search/fetch) and
  record the URL; if it cannot be verified, delete the claim rather than
  ship it.
- V0 (`ds/vendoring-infra`) adds the `vendored` flag. If the gate fails only on it before V0 merges, say so and send
  READY-TO-MERGE; you will get REBASE after V0.
- Skills attach by name to the Swift teammates (unit P-Swift). Never add
  them to a `Phase`. Copy only skill content (SKILL.md, references/*.md):
  no plugin.json, gemini-extension.json, images or dotfiles.

## FILES

own: the `skills/<name>/` dirs listed in STEPS, their entries in
`skills/provenance.json` (sorted by name), their rows in a "Swift and Apple
skills" table in `skills/README.md` (create it if missing; keep it sorted),
and `ai_docs/reports/domain-skills/<unit>.md`.

do not touch: Rust code, teammates, other skills.

## DONE WHEN

- `horch skills` (the built binary) lists every skill in this unit.
- The gate is green (or fails only on the V0 items above).
- The report has, per skill: verdict applied, every edit made with
  file:line, every "checked"/"verify" item with its evidence, and bytes.

## STEPS

0. Create the worktree.
1. Vendor `swift-testing-pro` (top-level copy `Swift-Testing-Agent-Skill/swift-testing-pro/`,
   not its nested `skills/`; add the optional headless-run line) and
   `observability` (`Observability-Agent-Skill/observability/`).
2. Adapt `swiftui-pro` (top-level copy, not the nested `skills/swiftui-pro/`).
3. Adapt `swift-concurrency-pro`: fix `testing.md:35` (`.serialized` on a
   suite serializes every test), add the NonisolatedNonsendingByDefault note,
   add build-settings table (`Swift-Concurrency-Agent-Skill/skills/swift-concurrency/SKILL.md:16-24`)
   as a reference with its own provenance source.
4. Adapt `swiftdata-pro`; add `migrations-and-history.md` and
   `core-data-adoption.md` (`swiftdata-agent-skill/swiftdata-expert-skill/references/`)
   minus the `#Unique` advice that lacks a CloudKit caveat (or add the caveat).
5. Adapt `swift-format-style` (fix `anti-patterns.md:80-88`).
6. Adapt `swiftui-liquid-glass` (`Skills/swiftui-liquid-glass/`):
   the four fixes; verify `scrollExtensionMode`.
7. Commit per skill or per two. Gate. Report.
