# E5: open the draft pull request to main and record the CI of each OS

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/ci-final.md && grep -q '^## Windows' docs/specs/fleetd/gates/ci-final.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/ci-final.md`, `docs/specs/fleetd/gates/fleetd-pp.md`, `docs/specs/fleetd/gates/fleetd-p1.md`, `scripts/pl6/windows-gaps.txt`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (unit E5, item L12).

## Goal

After the merge of `origin/main`, the run branch has a draft pull request to `main` whose CI result is recorded for macOS, Linux and Windows. The rows of PL-1, PL-3 and PL-6 in the gate files get their CI run id.

## Warning

A merge of this pull request with a CHANGELOG line publishes a release at once. The pull request must be a draft. Do not mark it ready. Do not merge it. Do not edit `CHANGELOG.md`.

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

1. Confirm `origin/main` is an ancestor of the run branch: `git fetch origin && git merge-base --is-ancestor origin/main HEAD`. If not, send BLOCKED.
2. Look for an open pull request: `gh pr list --head fleet/fleetd-finish --base main --state open`. If none exists, open it: `gh pr create --draft --base main --head fleet/fleetd-finish --title "fleetd: the supervised control loop (draft, do not merge)" --body "Draft. Not ready. See docs/specs/fleetd/design.md and docs/specs/fleetd/gates/."`.
   - Check: `gh pr view <n> --json isDraft` gives `true`.
3. Wait for the CI: `gh pr checks <n> --watch`.
4. Record each job result and each failure in `docs/specs/fleetd/gates/ci-final.md`, with the classes of `docs/specs/fleetd/gates/ci-baseline-2026-11.md`.
5. Read `docs/tickets/fleetd-pp/17-*.md` and `docs/tickets/fleetd-p1/29-*.md`. Write the CI run id of Linux and Windows into the gate rows of PL-6, PL-1 and PL-3 in `docs/specs/fleetd/gates/fleetd-pp.md` and `fleetd-p1.md`. Write `scripts/pl6/windows-gaps.txt` as ticket pp/17 says.
6. Commit with the subject `E5: the CI result of the draft pull request on 3 OSes`.
7. If a job is red, the report lists each failure and you send `horch tell orchestrator "NOTE: CI red: <count> failures, see docs/specs/fleetd/gates/ci-final.md"` before `horch ledger finish`.

## Out of scope

- Do not fix a CI failure in this ticket.
- Do not mark the pull request ready. Do not merge.

## Done when

The Check passes, the draft pull request exists, and each job result is in the report.
