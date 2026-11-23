# Scope report: what fleetd needs to run its workers on the second pane host (hc: herdr-core and the Herdr desktop app)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/hc-adapter-scope.md && grep -q '^## Units' docs/specs/fleetd/gates/hc-adapter-scope.md && grep -q '^## Options for the operator' docs/specs/fleetd/gates/hc-adapter-scope.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/hc-adapter-scope.md`

## Why

`origin/main` released v0.5.2 with PR #363: a second pane host backend (`hc`: herdr-core and the Herdr desktop app) for `horch fleet`, `horch spawn`, the courier, the doctor and the watchdog. The trial merge of main into the fleetd branch (task T-330, `docs/specs/fleetd/gates/merge-main-proof-2.md` on the run branch, section H2 and "For the operator") found: fleetd runs its workers only on `herdr`; after the merge `horch fleet` and `horch fleetd` refuse `hc` with a clear message; 6 tests of main in `hc_fleet_start.rs` were removed in the trial; the docs of main still promise `hc` fleets. So a merge of the fleetd pull request takes away a feature that main released. The operator must decide. Give them the facts and the size of the work.

## Steps

1. Read section H2 and "For the operator" of the proof file, the PR #363 (`gh pr view 363`; `git show 50e63b2a --stat`), `docs/specs/pane-host.md` and `docs/adr/0003-two-pane-hosts-behind-one-handle.md` of main (`git show origin/main:<path>`), and the trial code: `git worktree add` is not needed; read with `git show keep/fleetd-finish-trial-merge-main:<path>`.
2. Map each place where fleetd talks to the pane host: the launch of a worker (the launcher and the launch helper), the stop of an attempt, the liveness and the screen read of the stall ladder, the pane close, the workspace and tab layout, the watchdog duties, the doorbell and the mail courier, `horch doctor`, the conformance job, the test harness (`fake-herdr`, and the fakes `fake-hc` and `fake-herdr-app` of main). For each: the function, what it calls on `herdr` now, whether the handle of #363 (the 1 pane host handle of ADR 0003) already gives the same call for `hc`, and what is missing.
3. Security: FR-10 and SR-8 depend on how fleetd knows the process of a worker (the wrapper that fleetd records before the agent starts, the host PID, the parent and the group of the peer). Say for `hc` how a pane process is started and who its parent is, and if the wrapper record and the peer check work the same way. A gap here is a blocker, not a detail.
4. Write `docs/specs/fleetd/gates/hc-adapter-scope.md`: `## Facts` (what works on `hc` after the merge and what is refused); `## Gaps` (a table from step 2); `## Security` (step 3); `## Units` (the work as tickets that 1 worker can do each: title, files, the test that proves it, a size of S, M or L, and what blocks what); `## Options for the operator`: (a) merge fleetd with `hc` refused and say so in the release notes and the docs; (b) build the `hc` adapter before the merge (the units); (c) keep the old claim-and-spawn flow of main alive only for `hc` (say what that costs: 2 control paths, which rules of the design it breaks). Give the facts for each option; label each claim verified or not verified. Do not choose for the operator; say which option the design text supports and why.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Run no test suite: this task reads and writes text. `cargo fmt --all --check` is the only cargo command.
2. Never start the real Herdr desktop app or the real `hc`. Never touch the herdr of the operator or a process of another fleet.
3. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `/fleet`.
4. Do not edit code, `docs/specs/fleetd/design.md` or a pull request. Do not push a branch other than through `horch ledger finish`.
5. Write 1 `horch ledger progress` line each 10 minutes.

## Done when

The Check passes and each gap has a unit or a reason why it needs none.
