# U01 a0-gate: baseline record, gate script, coverage script, dependency allowlist

Unit slug: `a0-gate`. Branch: `ard/a0-gate`. Phase: A0.

## GOAL

`just gate` exists and passes on the base, `check-req-coverage.sh` supports
phases, the sha2 allowlist (OD6) is in place, the telemetry render goldens are
blessed in their own commit, and `BASELINE.md` records the baseline results.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in the master plan (`00-master-plan.md`): "Hard constraints", §3 "A0
  Baseline", §4 "NFR" table, §5 "Verification".
- Other A0 units run at the same time:
  - U02 `a0-oracles` writes oracle tests and fixtures.
  - U03 `a0-designs` writes the two design docs and `SPEC-COVERAGE.md`.
  - U04, U05, U06, U07, U08 start later phases in parallel. Several of them
    add `sha2` to a crate. Your allowlist change must land first, so work on
    §STEPS 4 early and keep it small.
- The new design docs (U03) use requirement tables with exactly this header:
  `| ID | Requirement | Phase | Tests |`. The Phase cell starts with one phase
  token: `A0`..`A12` or `B1`..`B6`. A cell can hold more, such as `A6/A12` or
  `B3–B5`. The first token is the phase of the ID.
- The old design `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` has
  requirement tables with no Phase column, and it repeats IDs in a later
  test-mapping table (for example NFR-01 at line 107 and line 725).

## FILES

own:
- `scripts/phase-gate.sh` (new)
- `scripts/check-req-coverage.sh`
- `scripts/check-deps.sh`
- `justfile` (add the `gate` recipe only)
- `crates/horch-core/tests/nfr.rs`
- `crates/horch/tests/golden/telemetry-*.txt` (new, blessed)
- `crates/horch-e2e/tests/golden/telemetry-e2e.txt` (new, blessed)
- `ai_docs/gates/architecture-refactor/BASELINE.md` (new)
- `ai_docs/gates/architecture-refactor/CHECKLIST.md` (new)
- `ai_docs/gates/architecture-refactor/CURRENT_PHASE` (new)
- `ai_docs/reports/arch-refactor-dataset/a0-gate.md` (new)

do not touch: any file under `crates/*/src/`, `ai_docs/designs/`, any oracle
fixture, `Cargo.toml` files.

## STEPS

1. Create the worktree (conventions §2). Check: `git log --oneline -1` shows
   the integration head.
2. Run the baseline commands from conventions §4 one at a time, with
   `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1`. Also run
   `./scripts/check-req-coverage.sh`. Record each command, exit code, test
   count and wall time in `BASELINE.md`. Record `rustc --version`,
   `cargo --version`, `git --version`, `sqlite3 --version`, and the base SHA.
   Check: `BASELINE.md` has one row per command.
   - If a baseline command fails on the untouched base, do not fix source.
     Record the failure, then send `BLOCKED:` with the failing test names.
3. Bless the missing telemetry render goldens. Find the tests that read
   `crates/horch/tests/golden/telemetry-*.txt` and
   `crates/horch-e2e/tests/golden/telemetry-e2e.txt` (`HORCH_BLESS` in
   `crates/horch/src/cmd/telemetry.rs` and `crates/horch-e2e/tests/scenario.rs`).
   Run them once with `HORCH_BLESS=1`. Read every blessed file. Confirm it
   holds no absolute path from your machine, no time-dependent value, and no
   secret. Run them again without `HORCH_BLESS`; they must pass.
   Commit only these golden files: `A0: Bless telemetry render goldens`.
4. Dependency allowlist (OD6, NFR-06):
   - `scripts/check-deps.sh`: add `sha2` to `allowed_core`. Add a third check
     for crate `horch-marketplace` with the allowed list
     `anyhow serde serde_json serde_yaml sha2`. Skip that check when the crate
     does not exist (`cargo tree -p horch-marketplace` fails). Update the
     header comment to name NFR-05 and NFR-06.
   - `crates/horch-core/tests/nfr.rs`: add `sha2` to `allowed_core` in
     `nfr_05_no_new_runtime_crates`. Add `nfr_06_dependency_allowlist`: it
     checks `crates/horch-marketplace/Cargo.toml` (skip if absent) against the
     list above, and checks that no workspace `Cargo.toml` names `reqwest`,
     `ureq`, `hyper`, `isahc`, `attohttpc` or `curl`.
   - Add `nfr_09_no_async_runtime_deps`: no workspace `Cargo.toml` names
     `tokio`, `async-std`, `smol`, `futures`, `rusqlite`, `sqlx` or `diesel`.
   - Add `nfr_11_no_proptest`: no workspace `Cargo.toml` names `proptest`
     or `quickcheck`.
   - Add `nfr_08_phase_gate_runs_every_check`: read `scripts/phase-gate.sh`
     and assert that it contains each command of master plan §5 "just gate".
   - Check: `cargo test -p horch-core --test nfr` passes.
   - Commit: `A0: Allowlist sha2 and add NFR-06/08/09/11 checks`.
5. Write `scripts/phase-gate.sh` (bash, `set -euo pipefail`, `cd` to the repo
   root). It runs, in this order, and stops at the first failure:
   - `cargo fmt --all --check`
   - `cargo build --workspace --all-targets`
   - `cargo build --workspace --bins`
   - `cargo test --workspace`
   - `HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
   - `scripts/check-req-coverage.sh` (no arguments: it reads `CURRENT_PHASE`)
   - `scripts/check-deps.sh`
   - `scripts/verify-telemetry-e2e.sh`
   It prints one header line per step and a final `GATE GREEN` line. It passes
   `HORCH_REQUIRE_GIT` and `HORCH_REQUIRE_SQLITE` through unchanged.
   Add to `justfile`, next to `verify`:
   ```
   # The per-commit gate for the arch-refactor-dataset branch
   # (ai_docs/plans/arch-refactor-dataset/00-master-plan.md, section 5).
   gate:
       ./scripts/phase-gate.sh
   ```
   Check: `just gate` prints `GATE GREEN`.
6. Generalize `scripts/check-req-coverage.sh`. Keep its current behavior for
   explicit ID arguments. New behavior:
   - Scan every `ai_docs/designs/*.md` for rows that match `^\| [A-Z]{3}-[0-9]{2} \|`.
   - A row is a definition only when it is in a table whose header row's
     second cell is `Requirement` or `Check`. Other tables (test-mapping
     tables) are references, not definitions.
   - If one ID has 2 or more definitions across all design files, print
     `DUPLICATE <ID>: <file:line> <file:line>` and exit 1.
   - If the table header has a cell named `Phase` or `Ph`, the ID's phase is
     the first token of that cell (`A0`..`A12`, `B1`..`B6`). Otherwise the ID
     has no phase (a legacy ID).
   - Phase order: A0 A1 A2 A3 A4 A5 A6 A7 A8 A9 A10 A11 A12 B1 B2 B3 B4 B5 B6.
   - Modes:
     - no arguments: check legacy IDs, plus IDs whose phase is listed in
       `ai_docs/gates/architecture-refactor/CURRENT_PHASE`. That file holds
       one phase per line. Ignore blank lines and lines that start with `#`.
       If the file is absent, check legacy IDs only.
     - `--through <P>`: check legacy IDs, plus IDs whose phase is at or
       before `<P>` in the phase order.
     - `--phase <P>[,<Q>...]`: check only IDs of these phases.
     - `<ID> ...`: check only these IDs (current behavior).
   - The test list comes from `cargo test --workspace -- --list` as today.
   - Check with a temporary copy of a design file that has a duplicate ID:
     the script exits 1. Remove the temporary file afterwards.
   - Check: `./scripts/check-req-coverage.sh` exits 0 on the base.
7. Create `ai_docs/gates/architecture-refactor/CURRENT_PHASE` with exactly
   these 2 lines:
   ```
   # One landed phase per line. The orchestrator appends a phase when it merges the last unit of that phase.
   ```
   (one comment line, then a trailing newline; no phase yet).
8. Write `ai_docs/gates/architecture-refactor/CHECKLIST.md`, the junior
   checklist for every phase. Mark it `Status: provisional until the Spec A
   §16 text arrives`. Make it a list of checkbox lines that a commit body can
   quote. Include at least:
   - `just gate` is green on this commit.
   - Every ID of this phase has a test (`check-req-coverage.sh --phase <P>`).
   - No golden prompt changed; any prose change is a named sanctioned block.
   - No serialization golden re-blessed; a format change bumped its schema version.
   - No new crate outside the allowlist.
   - No new `std::env` read outside `runtime/` and the binary's bootstrap.
   - No `ANTHROPIC_API_KEY` reaches a child process.
   - Every moved module left a re-export shim (until A12).
   - Old ledgers, old briefs and old teammate frontmatter still load.
   - The diff was re-read adversarially; the unit report lists gotchas.
9. Write the report `ai_docs/reports/arch-refactor-dataset/a0-gate.md`.
   Commit steps 5 to 9 as `A0: Add phase gate and phase-aware coverage check`.
10. Follow conventions §6 to finish.

## CONSTRAINTS

- Bash only for scripts; macOS ships bash 3.2, so do not use associative
  arrays or `mapfile`. Use `awk` for table parsing.
- Do not edit any source file under `crates/*/src/`.

## DONE WHEN

- `just gate` prints `GATE GREEN` on your branch.
- `./scripts/check-req-coverage.sh --phase A1` runs and exits 0 (no A1 IDs exist yet).
- The duplicate-ID check exits 1 on a duplicate and 0 on the base.
- `cargo test -p horch-core --test nfr` passes with the 4 new tests.
- `BASELINE.md`, `CHECKLIST.md` and `CURRENT_PHASE` exist.
- The telemetry goldens are committed in their own commit.

## REPORT

- `horch note` after steps 2, 3, 4 and 6.
- Send `BLOCKED:` at once if the base fails any baseline command.
- When step 4 is committed and green, send
  `horch tell orchestrator "[<role>] NOTE: allowlist commit <sha> is ready"`.
  The orchestrator can merge it early, because other units need it. Continue
  with step 5 without waiting.
- Send the READY-TO-MERGE line from conventions §6 once, after step 9.
