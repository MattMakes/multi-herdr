# P-Swift swift-teammates: the Swift and Apple teammates

Unit slug: `swift-teammates`. Branch: `ds/swift-teammates`.
Starts after S1–S4, V1 (`skill-fields`) and V2 (`roster-offer`) are merged
into `design-skills`.

## GOAL

The six teammates of `ai_docs/reports/swift-fleet-skills-2026-10.md` §6,
plus `codex-swift-reviewer` (§1 "Optional seventh"), exist, pass `--check`,
carry the §5 house style and their first-move toolchain check, and are
offered only on Apple projects.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, report §1,
  §5, §6, §7; `teammates/README.md`, `_template.md`, `architect-reviewer.md`,
  `qa-engineer.md`, `frontend-developer.md` (the "look at the running UI
  before DONE" pattern), `codex-reviewer.md` (the Codex reviewer shape).
- Use the §6 YAML sketches. Changes the fleet has settled:
  - `offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]` on all
    seven (V2), and whatever V2 added for the `horch doctor` xcode check.
  - `swift-developer` and `apple-platform-developer` may list
    `operator_skills: {dir: ~/.agents/skills, names: [swiftui-whats-new-27]}`
    (V1) only if `--check` passes on a host without that dir. If V1 makes a
    missing dir a hard error, leave it out and document in the persona
    comment how the operator turns it on. Decide and report.
  - XcodeBuildMCP: confirm the package name and launch args (`npx -y
    xcodebuildmcp@latest mcp`) against its README (web fetch), and pin a
    version instead of `@latest` (fleet rule: reproducible launches).
  - `app-release-preparer`: write it. Its `env:` carries a key *path*, never
    a key. Verify that `~` expands in `env:` (read the code); if it does
    not, fix the expansion in horch, or use a form that works, and test it.
    Its persona's first move: if the key file is missing, report BLOCKED.
    Check the `asc` deny patterns against `asc --help` if `asc` is
    installed; otherwise say so in the report and in a comment.
  - From `ai_docs/reports/domain-skills/swift-ship.md`: add
    `"Bash(asc metadata push *)"` to `app-release-preparer`'s
    `disallowed_tools` (push writes live metadata without `--confirm`).
    `-allowProvisioningUpdates` can change provisioning profiles in the
    developer account: the persona must not pass it unless the assignment
    names it, and a deny pattern for it is a second line of defence.
  - `codex-swift-reviewer`: agent codex, model and effort as §1 says (check
    the model name exists in the codex harness's known models), skills
    `swiftui-pro`, `swift-concurrency-pro`, read-only like `codex-reviewer`.
- The §5 rules (and the `twostraws/SwiftAgents` list, in our own words) go in
  the persona bodies of the builders and the reviewers.
- Count-dependent tests: the same list as P-UE
  (`ai_docs/plans/domain-skills/p1-ue-teammates.md` CONTEXT). The Codex
  count also changes.

## FILES

own: the seven `teammates/*.md`, the test lists, `teammates/README.md` (a
Swift section), V-unit fixes only where you must (say so),
`ai_docs/reports/domain-skills/swift-teammates.md`.

## STEPS

0. Create the worktree.
1. `swift-developer`, `swift-reviewer`, `swift-qa-engineer`. Check. Gate. Commit.
2. `apple-platform-developer`, `apple-accessibility-auditor`,
   `codex-swift-reviewer`. Check. Gate. Commit.
3. `app-release-preparer` plus the `env:` `~` check. Gate. Commit.
4. Show the `offer_when` behaviour with a temp `Package.swift` dir, as P-UE
   does. Report.
