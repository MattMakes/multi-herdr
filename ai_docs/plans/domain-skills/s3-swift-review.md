# S3 swift-review: review, test, accessibility and security skills

Unit slug: `swift-review`. Branch: `ds/swift-review`.

## GOAL

Bundle the report's §4.3 adapted skills: swiftui-performance-audit,
swift-security-expert, swift-code-audit, swiftdata-testing,
ios-simulator-run, swiftui-accessibility-auditor,
uikit-accessibility-auditor, appkit-accessibility-auditor and
writing-for-interfaces.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, then
  `ai_docs/reports/swift-fleet-skills-2026-10.md` §4 (your rows), §5 (house
  style: every adaptation must agree with the "fleet answer" column) and
  §7.3 (provenance).
- Sources: `/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/<owner>_<repo>/`; pins in `/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/PINS.txt`. The
  LICENSE for each skill is its repository's root `LICENSE`.
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
- V0 (`ds/vendoring-infra`) adds the `vendored` flag and accepts `LICENSE`
  files. If the gate fails only on those before V0 merges, say so and send
  READY-TO-MERGE; you will get REBASE after V0.
- Skills attach by name to the Swift teammates (unit P-Swift). Never add
  them to a `Phase`. Copy only skill content (SKILL.md, references/*.md,
  LICENSE): no plugin.json, gemini-extension.json, images or dotfiles.

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
1. The three rgmez auditors (`rgmez_apple-accessibility-skills/skills/*`):
   path fix only. Also copy `swift-accessibility-skill`'s
   `nutrition-labels.md` / `performAccessibilityAudit` material into the
   swiftui auditor's references only if the source is among the pinned
   repos; it is not, so skip it and say so.
2. `swiftui-performance-audit` (`Dimillian_Skills/`): worker records
   Instruments itself (`xcrun xctrace record --template SwiftUI`) or
   messages the orchestrator; `Text(value, format:)`.
3. `swift-security-expert` (`ivan-magda_swift-security-skill/`): cut about
   40% (tone, scope, self-review); decide on the three optional references
   and say why.
4. `swift-code-audit` from `jazzychad_ios-code-audit`: three sequential
   passes, report to an orchestrator-chosen path, drop the
   `swiftui-expert-skill` step, keep the four named parts.
5. `swiftdata-testing` (`akshaypimprikar_...`): fix
   `decimal-money-values.md:8-23`; make the MVVM section conditional (§5).
6. `ios-simulator-run` from `Dimillian_Skills/ios-debugger-agent`: a short
   playbook over XcodeBuildMCP with neutral tool names; boots a simulator
   itself; failures go to the orchestrator.
7. `writing-for-interfaces` (`andrewgleave_skills/`): drop `context: fork`,
   description to about 250 characters, voice inferred from the codebase or
   asked of the orchestrator.
8. Commit per skill or two. Gate. Report.
