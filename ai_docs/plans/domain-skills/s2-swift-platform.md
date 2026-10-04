# S2 swift-platform: Apple system-surface skills

Unit slug: `swift-platform`. Branch: `ds/swift-platform`.

## GOAL

Bundle the report's §4.2 vendored skills: app-intents, widgets,
background-execution, swift-focusengine-pro.

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
1. Vendor `app-intents` (`App-Intents-Agent-Skill/app-intents/`),
   `widgets` (`Widgets-Agent-Skill/widgets/`), `background-execution`
   (`Background-Execution-Agent-Skill/background-execution/`).
2. Vendor `swift-focusengine-pro` (`Swift-FocusEngine-Agent-Skill/`,
   repo root; the directory gets the skill name). If horch's frontmatter
   parser rejects `version`/`author`/`tags`, move them under `metadata`
   and record that as the only adaptation.
3. Verify the iOS 27 claims the report flags (`widgets`
   `allowedExecutionTargets`, `app-intents` `AppIntentsTesting`,
   `LongRunningIntent`) against Apple docs; record the result. Do not edit
   a verbatim skill to fix them: if one is wrong, note it in the report and
   in the skill's provenance `notes`, and tell the orchestrator.
4. Skip `macos-spm-app-packaging` (the fleet does not ship macOS apps
   outside the App Store yet). Say so in the report.
5. Gate. Commit. Report.
