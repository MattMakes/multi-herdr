# F6: the Phase 7 gate and the final gate

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && test -s docs/specs/fleetd/gates/fleetd-p7.md && test -z "$(cat crates/horch-e2e/expected-failures/*.txt 2>/dev/null | grep -v '^#' | grep -v '^$')" && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/`, `crates/horch-e2e/expected-failures/`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (unit F6).

## Goal

The gate of Phase 7 and the final gate pass as `docs/tickets/fleetd-p7/36-gate.md` says: each design id maps to a passing test, each expected-failures list is empty, and each CI-only row and each open operator question is recorded.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Read `docs/tickets/fleetd-p7/36-gate.md`. Its steps are the steps of this ticket. Where it names a command without the sandbox, run that command through `scripts/dev-sandbox.sh`.
2. Read the gate files `docs/specs/fleetd/gates/fleetd-p4.md`, `fleetd-p5.md`, `fleetd-p6.md`, `ci-final.md` and each `docs/specs/fleetd/reviews/end-review-*.md`.
3. For each id of the table "Where each rule becomes green" of the design, write 1 row in `docs/specs/fleetd/gates/fleetd-p7.md`: id, test name, result of this run, CI run id when the row needs another OS.
4. Add `## Open operator questions`: Q1 to Q4 of the spec, each with its answer from `ai_docs/plans/fleetd-finish/decisions.md` or the word "open".
5. Add `## Review findings`: each Critical and Important finding id of the end reviews, with its fix commit or its waiver.
6. If a row is red or a list is not empty, send BLOCKED with the row. Do not remove an entry of a list to pass the Check.

## Out of scope

- Do not change product code. A red row is a new ticket of the orchestrator.

## Done when

The Check passes and `fleetd-p7.md` has 1 row for each design id.
