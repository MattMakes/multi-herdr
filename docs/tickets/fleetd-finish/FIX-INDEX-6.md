# Fix tickets index: end review R6 (Phase 7)

Review covered:
- R6: `docs/specs/fleetd/reviews/end-review-p7.md` (on `fleet/fleetd-finish`)

## Finding R6-F3 — Already fixed on `fleet/fleetd-finish` by T-16 (ticket `16-d1-antigravity-stop-hook.md`, commits `b1382d1a` and `df3f9299`)

- **R6-F3 (Important — FM-6: Antigravity interactive attempts have no Stop gate)**:
  - Review text: R6 reviewed `fleet/fleetd-finish` before task T-16 landed and noted that `crates/horch-core/tests/harness_defaults.rs:761` defined and asserted Stop hooks only for Claude and Codex, while no Antigravity Stop mechanism was implemented.
  - **Why no ticket is written**: Per the extra rules of `fix-tickets-brief-6.md`, task T-16 (`16-d1-antigravity-stop-hook.md`) has already landed on `fleet/fleetd-finish` in commits `b1382d1acd288c9a8f798ddfb5e4bac62e29b9a1` and `df3f92996b9b460de0eb4f89045eb5a9b1be421a`:
    - **Code proof**: In `crates/horch-core/src/harness/antigravity.rs:76-164` and `:210-212`, `Antigravity::command` calls `seed_stop_hook(worktree, &env.horch_exe)` whenever `env.task_dir.is_some()` and `env.worktree.is_some()` (a push attempt). `stop_hook_command` (`antigravity.rs:87-95`) runs `stop_gate::hook_command(horch_exe)` (`'<horch>' result --check`) and maps exit code `2` to stdout JSON `{"decision":"continue","reason":...}` and any other exit code to `{"decision":"stop"}`. `seed_stop_hook` writes `.agents/hooks.json` in the worktree, adds `/.agents/hooks.json` to `<git-common-dir>/info/exclude`, preserves tracked `.agents/hooks.json` files, and refuses to write through symlinks at `.agents` or `.agents/hooks.json`. Moreover, `crates/horch-core/src/harness/antigravity.rs:374-383` (`pub fn operator_boundary`) continues to return `Err(NotSupported)` on every OS, so interactive and exec Antigravity worker profiles remain explicitly refused by SR-8 profile validation until Antigravity has an Operator boundary.
    - **Test proof**:
      - `crates/horch-core/src/harness/antigravity.rs:574` (`tests::the_stop_hook_maps_exit_2_to_continue`)
      - `crates/horch-core/tests/harness_defaults.rs:725` (`antigravity_push_launch_has_a_stop_hook_that_runs_result_check`)
      - `crates/horch-core/tests/harness_defaults.rs:750` (`antigravity_push_launch_keeps_a_tracked_hooks_file`)
      - `crates/horch-core/tests/harness_defaults.rs:772` (`antigravity_push_launch_writes_through_no_symlink`)
      - `crates/horch-core/tests/harness_defaults.rs:790` (`claim_attempt_has_no_stop_hook`)
  - Nothing remains unimplemented for R6-F3.

## Fix tickets

| File | Finding ID | Rank | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [80-fix-r6-f1-conformance-health-fail-closed.md](80-fix-r6-f1-conformance-health-fail-closed.md) | R6-F1 | Critical | primary `opus-hardening`, fallback `opus` | `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/tests/fleetd_pass_p7.rs` | the landing chain (T-15) |
| [81-fix-r6-f2-request-write-precondition-probe.md](81-fix-r6-f2-request-write-precondition-probe.md) | R6-F2 | Important | primary `opus-hardening`, fallback `opus` | `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`, `crates/horch-core/tests/p7_operator_boundary.rs` |  |
| [82-fix-r6-f4-real-cli-conformance-release-gate.md](82-fix-r6-f4-real-cli-conformance-release-gate.md) | R6-F4 | Important | primary `opus-hardening`, fallback `opus` | `scripts/live/p7-real-conformance.sh`, `crates/horch-e2e/tests/p7_conformance_binary.rs` |  |

## Overlap and `pass.rs` notes

- **No overlap with `FIX-INDEX-1.md` or `FIX-INDEX-2.md`**: None of R6-F1, R6-F2, R6-F3, or R6-F4 is the same defect as any ticket in `FIX-INDEX-1.md` (tickets 35 to 39) or `FIX-INDEX-2.md` (tickets 50 to 53), and none of tickets 80 to 82 shares an `Own` path with tickets 35 to 39 or 50 to 53.
- **No shared files among tickets 80 to 82**: Tickets 80, 81, and 82 own completely disjoint file sets.
- **`pass.rs` blocker on ticket 80 (R6-F1)**: Because R6-F1 requires moving job observation and `p7_rows` (`conformance_results`) before `dispatch_rows` in `Pass::run` (`crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1313-1325`) and updating `act_on_conformance` (`pass.rs:4241-4274`), ticket 80 must own `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` and is marked `Blocked by: the landing chain (T-15)`. Tickets 81 and 82 do not touch `pass.rs`.

## Minor findings

- **R6 (`end-review-p7.md`)**: None (0 Minor findings).

## Tests that do not prove their rule

Every point in the `## Tests that do not prove their rule` section of `docs/specs/fleetd/reviews/end-review-p7.md` is covered by the finding tickets above or by the already-landed T-16 commit on `fleet/fleetd-finish`, so no separate `qa-engineer` ticket is needed:

1. **SR-8 Operator boundary — not verified for real CLIs** (`crates/horch-core/tests/p7_operator_boundary.rs:1` launches fake agents): covered by ticket [82-fix-r6-f4-real-cli-conformance-release-gate.md](82-fix-r6-f4-real-cli-conformance-release-gate.md) (R6-F4).
2. **SR-8 request write — not verified** (`operator_boundary_fails_when_the_rules_allow_a_request_write` at `crates/horch-core/tests/p7_operator_boundary.rs:392` starts with a writable fixture and does not test an unwritable host folder): covered by ticket [81-fix-r6-f2-request-write-precondition-probe.md](81-fix-r6-f2-request-write-precondition-probe.md) (R6-F2, test `operator_boundary_fails_when_the_request_folder_is_not_writable`).
3. **FM-19 health application — not verified for ordering and corrupt output** (existing cases do not prove that health changes before dispatch or that missing/malformed `conformance.json` fails closed): covered by ticket [80-fix-r6-f1-conformance-health-fail-closed.md](80-fix-r6-f1-conformance-health-fail-closed.md) (R6-F1, tests `failed_conformance_blocks_dispatch_in_the_same_pass` and `changed_cli_and_missing_or_corrupt_conformance_report_fail_closed`).
4. **FR-6 and FM-18 exec behavior — not verified for installed binaries** (`crates/horch-core/tests/p7_wrapper_exec.rs:30` uses fake CLIs): covered by ticket [82-fix-r6-f4-real-cli-conformance-release-gate.md](82-fix-r6-f4-real-cli-conformance-release-gate.md) (R6-F4).
5. **FM-6 Stop gate — not verified for Antigravity** (`crates/horch-core/tests/harness_defaults.rs:761` had no Antigravity Stop-hook case when R6 ran): already fixed on `fleet/fleetd-finish` by T-16 (commits `b1382d1a` and `df3f9299`), which added `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`, `antigravity_push_launch_keeps_a_tracked_hooks_file`, `antigravity_push_launch_writes_through_no_symlink`, and updated `claim_attempt_has_no_stop_hook` in `crates/horch-core/tests/harness_defaults.rs:725-805` plus `the_stop_hook_maps_exit_2_to_continue` in `crates/horch-core/src/harness/antigravity.rs:574`.

## Open questions

1. **Antigravity release support — decision required.** The reviewed implementation correctly refuses its profiles, and T-16 (`b1382d1a`, `df3f9299`) added the interactive Stop hook in `.agents/hooks.json`, but private HOME (for interactive mode / MCP rendering) and the Operator boundary (`crates/horch-core/src/harness/antigravity.rs:374`) are absent. Recommendation: list Antigravity as unsupported at release and do not accept its profiles until all rules pass.
2. **Real-CLI release gate — decision required.** Recommendation: require one isolated production conformance run for each supported Claude and Codex version before release (using `scripts/live/p7-real-conformance.sh` from ticket 82 under the operator's login without `ANTHROPIC_API_KEY`), and retain the report as release evidence. Fake-agent tests remain the deterministic CI layer.
3. **Docs-only sources — decision required for design wording.** The implementation gives herdr docs sync without conformance because it has no exec adapter (`crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs:17`). Recommendation: keep this behavior and state explicitly that docs-only sources do not create worker-profile health.
4. **Drift-idea project selection — decision required for design wording.** The implementation selects the first repository by path (`crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4276`). Recommendation: preserve the deterministic rule and add it to the design before release.
