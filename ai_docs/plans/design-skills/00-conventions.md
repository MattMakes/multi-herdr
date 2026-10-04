# Fleet conventions for the design-skills run

Every unit plan in `ai_docs/plans/design-skills/` points here. Read this file
completely before you start.

## 1. What this run builds

- Design skills: the operator gave 7 public skill repositories (all MIT).
  We combine and rewrite them into a small set of efficient bundled skills
  under `skills/<id>/`. We make them our own: rewritten, deduplicated,
  harness-neutral. We do not copy LICENSE files. We record provenance in
  `skills/provenance.json` (repository, revision, path, sha256, license).
- New design personas (teammates) that use those skills.
- A new harness: the Antigravity CLI agent.
- A new command: `horch agent-list`.

## 2. Sources (read-only)

- The 7 repositories are cloned, read-only, under
  `/Users/mascott/projects/mh-wt/_sources/design-skills/<repo>/`.
- `/Users/mascott/projects/mh-wt/_sources/design-skills/PINS.txt` gives each
  repository URL and the pinned commit. Use these commits in provenance.
- Any unit may READ any source. A unit WRITES only the skills its plan owns.
- Do not clone the repositories again. Do not run their scripts, installers,
  or package managers.

## 3. Git: worktrees and branches

- The orchestrator owns the integration worktree
  `/Users/mascott/projects/mh-wt/integ-ds` on branch `design-skills`.
  Do not edit, commit, checkout or build in that directory.
- Do not run `git checkout` or `git switch` in `/Users/mascott/projects/multi-herdr`.
- Create your own worktree from the integration branch:

  ```
  git -C /Users/mascott/projects/multi-herdr worktree add -b ds/<unit> /Users/mascott/projects/multi-herdr/.worktrees/<unit> design-skills
  cd /Users/mascott/projects/multi-herdr/.worktrees/<unit>
  ```

- Do all work in your worktree. Use absolute paths. Use your own
  `CARGO_TARGET_DIR` (the default `target/` inside your worktree). Never share
  a target dir with another worktree.
- Commit on `ds/<unit>` only. Do not push. Do not open a PR. Do not merge.
- Commit subjects: `<Area>: <imperative summary>`, for example
  `Skills: Add ui-taste`, `Teammates: Add design-critic`,
  `Harness: Add Antigravity adapter`, `CLI: Add horch agent-list`.

## 4. Shared files

- Edit only the files under `own` in your unit plan. To change another file,
  ask first with `QUESTION:`.
- `skills/provenance.json` and `skills/README.md` get entries from several
  units. Add your entries in alphabetical order by skill id. At rebase, keep
  both sides.
- `crates/horch-core/src/lib.rs`, `harness/mod.rs`, `cmd/mod.rs`, `main.rs`:
  add lines only; keep both sides at rebase.

## 5. The gate

Every commit must pass. Run from your worktree root:

```
HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate
```

- Oracles and goldens: `crates/horch-core/tests/oracles/` and
  `crates/horch-core/tests/golden/`.
  - A NEW teammate gets NO oracle files: add its name to
    `SKIP_NEW_TEAMMATES` in `crates/horch-core/tests/baseline_oracles.rs`
    and `skills_catalog.rs` (the A0 oracle counts are frozen). A new
    teammate also needs its phase arm and the Claude count in the 2 tests
    in `crates/horch-core/src/roster/validation.rs`.
  - An oracle or golden of an EXISTING teammate may change only when your
    plan says so. Then put the diff summary in your report.
  - Any other oracle or golden change: stop and send `QUESTION:` with the diff.
- Do not change existing tests to make them pass, unless your plan says so.

## 6. Hard rules

- NEVER read, print, set, export or pass `ANTHROPIC_API_KEY`. If you run
  `claude` yourself, use `env -u ANTHROPIC_API_KEY claude ...`.
- No new crates. No HTTP crate, no async runtime.
- Tests are hermetic: no network, no real harness binary, no herdr server.
- No subagents, no background agents, no nested agent CLIs
  (`claude -p`, `codex exec`, `gemini`, `opencode run`) unless your plan
  says so for research.
- Keep the style of the surrounding files.

## 7. Messages and the merge protocol

Write every message in Simplified Technical English.

- Progress: `horch note "<one line>"` after each major step.
- Questions: `horch tell orchestrator "[<role>] QUESTION: <question>"`. Give
  options and your recommendation. Continue with other steps while you wait.
- When your unit is complete:
  1. `git -C <your worktree> rebase design-skills`. Resolve conflicts.
  2. Run the gate again. It must be green.
  3. Send exactly one line:
     `horch tell orchestrator "[<role>] NOTE: READY-TO-MERGE ds/<unit> <short-sha>. Gate green. See <report path>."`
  4. Wait. On `REBASE`, repeat 1 to 4. On `MERGED`, run `horch done "<summary>"`.
- Write a report to `ai_docs/reports/design-skills/<unit>.md` and commit it.
  Include decisions, what you dropped and why, gotchas, and follow-ups.

## 8. Where worktrees live (from D16 on)

New unit worktrees go under `/Users/mascott/projects/multi-herdr/.worktrees/<unit>`
(git-ignored). Each has its own `target/` (2 to 8 GB). The orchestrator
removes a worktree as soon as its unit merges. Older units in
`/Users/mascott/projects/mh-wt/` finish where they are.
