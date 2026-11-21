# 82: Fix R6-F4: prepare the isolated real-CLI conformance release gate and operator step (FR-6, FR-8, SR-1, SR-8, FM-18, FM-19)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p7_conformance_binary && bash -n scripts/live/p7-real-conformance.sh && cargo fmt --all --check`
**Own:** `scripts/live/p7-real-conformance.sh`, `crates/horch-e2e/tests/p7_conformance_binary.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p7.md` (finding R6-F4) and `docs/specs/fleetd/design.md` (rules FR-6, FR-8, SR-1, SR-7, SR-8, FM-18, FM-19).

## Goal

Prepare a deterministic test and an isolated runner command `scripts/live/p7-real-conformance.sh` that runs the production conformance suite once with each supported real CLI (`claude` and `codex`) in an isolated temporary fleet root and retains `<out>/<cli>/<version>/conformance.json` as release evidence, with a dry-run / fake-backed verification test in `crates/horch-e2e/tests/p7_conformance_binary.rs` and explicit documentation of the operator step (since running real CLIs requires the operator login and costs quota, and the worker never uses `ANTHROPIC_API_KEY`).

## Requirements

- **R6-F4 (Important — FR-6, FR-8, SR-1, SR-8, FM-18, FM-19)**:
  - **Finding text and file:line**:
    SR-1, SR-8, FM-18 and FM-19 depend on behavior of installed agent binaries. The Operator-boundary test states that it launches fake agents and never a real agent CLI at `crates/horch-core/tests/p7_operator_boundary.rs:1`. The exec suite also identifies its commands as fakes at `crates/horch-core/tests/p7_wrapper_exec.rs:30`. These tests prove adapter argv, stream parsing and rendered sandbox mechanics, but not that real Claude and Codex honor the prompt, hooks, Stop gate, private home, exact MCP set, or sandbox boundary.
    Concrete case: a real CLI version changes a flag, hook format or child-process behavior while the fake continues to accept the adapter contract. The suite passes and marks the profile healthy although the protected behavior differs. The smallest fix is a release gate that runs the production conformance suite once with each supported real CLI in an isolated fleet root and retains its `conformance.json`.
  - **Design rules**:
    - **FR-6** (`docs/specs/fleetd/design.md:142`): "A worker profile shall declare CLI, model, skills, MCP servers, sandbox, writable paths, context budget, timeouts and restart policy."
    - **FR-8** (`docs/specs/fleetd/design.md:144`): "A docs-sync job shall fetch each CLI's documentation, record the installed version, and run adapter conformance tests on change."
    - **SR-1** (`docs/specs/fleetd/design.md:177`): "Each worker shall write only to its task directory and its worktree, plus paths its profile names."
    - **SR-7** (`docs/specs/fleetd/design.md:184`): "The launcher shall apply the environment rule of the profile, and shall always remove `ANTHROPIC_API_KEY`."
    - **SR-8** (`docs/specs/fleetd/design.md:182`): "The sandbox of each worker shall deny reading the token file, connecting to the command channel and writing into the request folder, and profile validation shall refuse a profile whose sandbox allows 1 of them."
    - **FM-18** (`docs/specs/fleetd/design.md:1563`): "Trust pre-seeded in each private config home; conformance check 'no trust prompt'."
    - **FM-19** (`docs/specs/fleetd/design.md:1564`): "Docs sync plus conformance on version change; failing CLI marked unhealthy."

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

1. Write the failing test first in `crates/horch-e2e/tests/p7_conformance_binary.rs`:
   - Add test function `real_conformance_gate_script_runs_isolated_and_retains_reports`:
     - Invoke `scripts/live/p7-real-conformance.sh --out <temp_evidence_dir>` with `PATH` prepended by the test's fake `claude` and `codex` wrapper directory (and `HORCH_BIN` pointing to the built `horch` binary, `HORCH_TEST_DOCS_FIXTURE` pointing to the local docs fixture, and `ANTHROPIC_API_KEY` unset).
     - Assert that the script exits 0, runs in an isolated temporary `FLEET_ROOT` / `HORCH_LEDGER_DB` / `HORCH_STATE_DIR` / `HORCH_DATA_DIR`, stops its temporary `fleetd` / channel listener before exiting, verifies that every check in `conformance.json` for `claude` and `codex` (including `Operator boundary`) has verdict `"pass"`, and retains `<temp_evidence_dir>/claude/<version>/conformance.json` and `<temp_evidence_dir>/codex/<version>/conformance.json`.
     - Also assert that when a check in `conformance.json` has verdict `"fail"` or `"not supported"`, the script exits non-zero and prints the failing check name.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p7_conformance_binary -- real_conformance_gate_script_runs_isolated_and_retains_reports` and see it fail because `scripts/live/p7-real-conformance.sh` does not exist yet.
2. Create `scripts/live/p7-real-conformance.sh` (mode `0755`):
   - Strip forbidden environment variables at startup: `unset ANTHROPIC_API_KEY GEMINI_API_KEY GOOGLE_API_KEY GOOGLE_GEMINI_BASE_URL`. Never read, set, or pass `ANTHROPIC_API_KEY`.
   - Accept `--out <evidence_dir>` (defaulting to a designated evidence folder) and optional `--clis "claude codex"`.
   - Create an isolated temporary directory for `FLEET_ROOT`, `HORCH_LEDGER_DB`, `HORCH_DATA_DIR`, and `HORCH_STATE_DIR` (cleaning up all temporary processes and directories on `EXIT` trap), seed the operator token (`0600`), request directory (`0700`), and a live command channel socket or temporary `fleetd` instance, and run the production conformance job (`horch job run <id>`) for each requested CLI (`claude`, `codex`).
   - Copy each resulting `<fleet_root>/docs/<cli>/<version>/conformance.json` into `<evidence_dir>/<cli>/<version>/conformance.json`, verify that every check in the report has `"verdict": "pass"`, and print the exact retained file paths and a summary table.
   - Include a clear header comment and `--help` output naming the **Operator release step**:
     - The worker never runs live real-CLI conformance with `ANTHROPIC_API_KEY` or unapproved quota spend.
     - Before release, the operator runs in their logged-in shell:
       `env -u ANTHROPIC_API_KEY scripts/live/p7-real-conformance.sh --out docs/specs/fleetd/gates/evidence/p7-conformance`
       and records the retained `conformance.json` paths in `docs/specs/fleetd/gates/fleetd-p7.md`.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Running live real-CLI turns against the operator's Claude or Codex subscription from the worker session (that is the operator step prepared by this ticket).
- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.

## Done when

`scripts/live/p7-real-conformance.sh` exists, `real_conformance_gate_script_runs_isolated_and_retains_reports` in `crates/horch-e2e/tests/p7_conformance_binary.rs` passes, the operator command is documented in the script, and the ticket Check passes.
