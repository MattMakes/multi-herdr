# F7: the release handoff for the operator

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/fleetd/release-handoff.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/release-handoff.md`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (unit F7).

## Goal

1 file tells the operator what to do to release fleetd, and what changes for a fleet that runs at the time of the update.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Read `docs/specs/fleetd/gates/fleetd-p7.md`, `ci-final.md`, each `docs/specs/fleetd/reviews/end-review-*.md`, and the design sections "Migration plan" and "What a release does to a run in flight".
2. Read `.github/workflows/auto-release.yml` and the skills `.claude/skills/push-pr` and `.claude/skills/release` when they exist, for the release steps of this repository.
3. Write `docs/specs/fleetd/release-handoff.md` with: `## State` (the branch tip, the pull request number, the CI run ids); `## Open decisions` (each open operator question and each waiver); `## Before you merge` (a numbered list: what to stop, what to back up, for example `~/.horch/ledger.sqlite3`); `## Merge and release` (the exact steps, the CHANGELOG line rule); `## What happens to a fleet in flight` (from the design table, for each phase); `## Rollback` (how to go back to the earlier release and ledger backup); `## Known limits` (Windows sandbox, agy, each Minor finding that stays).
4. Commit with the subject `F7: the release handoff`.

## Out of scope

- Do not merge, release or edit `CHANGELOG.md`.

## Done when

The Check passes and each section exists.
