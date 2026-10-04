# S4 swift-ship: App Store shipping skills (diagnosis and dry-run only)

Unit slug: `swift-ship`. Branch: `ds/swift-ship`.

## GOAL

Bundle the report's §4.4 skills: asc-cli-usage, asc-id-resolver,
asc-crash-triage, asc-xcode-build, asc-submission-health,
asc-metadata-sync, appstore-review and app-store-changelog. None of them
may lead a worker to change anything live on App Store Connect: every
mutation becomes "send the dry-run to the orchestrator".

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
1. The four light `asc-*` adaptations
   (`rudrankriyam_app-store-connect-cli-skills/skills/`): drop "in this
   repo", the web-auth and Apple Ads sections.
2. `asc-submission-health`: diagnosis half only.
3. `asc-metadata-sync`: dry-run only; state plainly that `asc metadata
   push` without `--confirm` still writes live metadata, so the worker
   never runs push.
4. `appstore-review` (`3paws-ai_mobile-ai-skills/skills/appstore-review/`):
   flatten the description, drop `trigger`/`agents` if horch's parser
   rejects them, runtime-only checks report UNVERIFIED, style rules WARN.
5. `app-store-changelog` (`Dimillian_Skills/`): the one-line edit. Its
   script is `.sh`: bundled files must be text-only `.md`/`.txt`, so
   inline the read-only `git log` command into SKILL.md instead.
6. grep every skill for `submit`, `push`, `delete`, `expire`, `revoke`,
   `cancel` and confirm each hit is diagnosis or orchestrator-gated; list
   the hits in the report.
7. Gate. Commit. Report.
