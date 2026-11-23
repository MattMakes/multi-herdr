# 1 file with each open decision of the operator, with the facts, the options and the recommendation

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/operator-decisions.md && grep -q '^## Decisions that block the merge' docs/specs/fleetd/gates/operator-decisions.md && grep -q '^## Decisions that do not block the merge' docs/specs/fleetd/gates/operator-decisions.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/operator-decisions.md`

## Why

Done criteria 1, 4 and 7 of the run need the operator to decide each waiver, each expected failure that stays and each change of the design text. The decisions are spread over a log of about 500 lines and over 12 report files. The operator needs 1 page.

## Sources (read all)

1. The decisions log: `/Users/matthewscott/repo/multi-herdr/docs/tickets/fleetd-finish/_decisions-snapshot-1004Z.md` (a copy of the log of the orchestrator; search for "OPERATOR", "operator decision", "open", "not verified", "Limit").
2. On the run branch, folder `docs/specs/fleetd/`: `reviews/end-review-a.md` (5 operator decisions), `reviews/delta-review-*.md` (3 files), `gates/hc-adapter-scope.md` (options a, b, c), `gates/merge-main-plan.md` and `merge-main-proof.md` and `merge-main-proof-2.md` (sections "For the operator"), `gates/pre-gate-p4.md`, `pre-gate-p5.md`, `pre-gate-p6.md`, `gates/ci-run-4.md`, `gates/macos-check-sandbox-lan.md`, `gates/linux-git-241-proof.md`, `gates/pr-description.md`.
3. `crates/horch-e2e/expected-failures/` (each entry that is left).
4. The goal: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/goal-prompt.md` (the done criteria and the list "Stop and ask the operator").

## Output: `docs/specs/fleetd/gates/operator-decisions.md`

For each decision 1 entry with an id (`OD-1` ...): the question in 1 sentence that needs no other file to understand; the facts (with the source file and section); the options (a, b, c) and what each costs; the recommendation of the run and its reason; what is blocked until the answer; the state of the code now (which option the branch has today). Sections: `## Decisions that block the merge` (a released feature of main that is lost, a security rule, a requirement with no test, an expected failure that stays), `## Decisions that do not block the merge` (limits that are documented, design text to update, release notes), `## Decided by the orchestrator, for the record` (rulings that changed behavior and that the operator may want to reverse: 1 line each with the task number). Known decisions that the file must hold (find the others): the second pane host hc of PR #363 under fleetd; the release and a CHANGELOG line; the comment-sweep step of main under fleetd; the macOS check sandbox and the LAN address (R8-F2); the 3 expected failures of `i308_fleet_steward`; FR-7 and SR-8 literal text with no test and Windows SR-5; the key `unlanded` of `result.json`; schema version numbers in the design text; the worktree state table and trees with no task; `ui-prop-tester` against PL-2 and SR-7; Windows as a release target of fleetd (no sandbox for git, the folder lock guard holds nothing, the hold of the termination handler does nothing); a KILL by fleetd of recorded orphan processes after a bound; the FM-22 stop of an orphan group after an id reuse; the 10 s limit of a local git and work over 1 s in the pass; a hold of dispatch for a profile with no conformance run yet; unhealthy review profiles; a real-CLI conformance run before the release; Antigravity out of the release claim; the new host needs (git 2.41, util-linux setsid on Linux, a fleet root not under /tmp on Linux).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. This task reads and writes text: `cargo fmt --all --check` is the only cargo command.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `/fleet`.
3. Do not edit code, `docs/specs/fleetd/design.md`, a pull request or its description on GitHub. No push other than through `horch ledger finish`. No name of a person and no email address in the file. No attribution line.
4. Label each claim "verified" with its evidence (a commit, a file and line, a report file) or "not verified". Write in Simplified Technical English: short sentences, 1 instruction or fact in each.

## Done when

The Check passes and each "OPERATOR" line of the log has an entry or a reason why it is closed.
