# U01 a0-gate report

Branch `ard/a0-gate`, phase A0, worker opus-3.

## What landed

- Baseline fixes, one commit per root cause; see
  `ai_docs/gates/architecture-refactor/BASELINE.md` for the failure table.
  The base went from 47 failing tests to 0.
- `A0: Bless telemetry render goldens`: 3 golden files, reviewed (no
  absolute paths, no secrets, only pinned fixture clocks).
- `A0: Re-freeze oracles after baseline fixes`: 1 routing oracle changed,
  every line traced to the `bal_03` fixture fix.
- `A0: Allowlist sha2 and add NFR-06/09/11 checks` (merged early as
  `8e90066`).
- `scripts/phase-gate.sh` and `just gate`; `nfr_08_phase_gate_runs_every_check`.
- `scripts/check-req-coverage.sh`: scans every `ai_docs/designs/*.md`,
  phase-aware, rejects duplicate IDs.
- `ai_docs/gates/architecture-refactor/{BASELINE.md, CHECKLIST.md, CURRENT_PHASE}`.

Tests added: 4 (`nfr_06_dependency_allowlist`, `nfr_08_phase_gate_runs_every_check`,
`nfr_09_no_async_runtime_deps`, `nfr_11_no_proptest`).

## Decisions

- `bal_03`: the fixture changed (codex `resets_at`), not the pace rule
  (orchestrator: option 1).
- `the_template_documents_exactly_the_teammate_fields`: the test now
  serializes a `Teammate` with 1 fallback. The struct keeps
  `skip_serializing_if`, so the brief format is unchanged.
- `quo_06`: `assess_pool` skips the refusal check for `POOL_ZEN`; a Zen
  refusal is a cooldown (QUO-06).
- NFR-06 HTTP-crate check exempts `herdr-install` and `herdr-docs-sync`.
  Both used `ureq` before the rule. The marketplace list check is skipped
  until `crates/horch-marketplace` exists.
- `phase-gate.sh` runs `cargo test --workspace --no-fail-fast`, so one run
  lists every failure. It runs `check-req-coverage.sh` with no arguments
  (reads `CURRENT_PHASE`), as the unit plan says. The master plan §5 writes
  `--through $(cat CURRENT_PHASE)`; with a comment line in that file that
  form does not work, so the no-argument mode implements it.
- `nfr_08` lands with `phase-gate.sh`, not with the allowlist commit, so
  every commit stays green.

## Coverage script semantics

- Definition rows: `| XXX-00 |` in a table whose header's second cell is
  `Requirement` or `Check`. Other tables are references.
- Phase: first token (`A0`..`A12`, `B1`..`B6`) of the `Phase`/`Ph` column.
  No such column -> legacy ID.
- No arguments: legacy IDs plus the phases listed in `CURRENT_PHASE`.
  `--through P`: legacy plus phases up to P. `--phase P[,Q]`: only those
  phases. IDs: only those IDs. Unknown phase or option: exit 2.
- Today: 162 definitions (38 legacy, 124 phased), 0 duplicates.

## Gotchas for later phases

- `--phase A1` exits 1 today: the merged design defines ARC-02..ARC-04 for
  A1 and they have no tests yet. The unit plan's "exits 0" predates the
  design docs. `--through A0` exits 0.
- Copying a macOS system binary into a temp dir makes it unrunnable
  (SIGKILL, 137). Symlink it.
- `sqlite3` reads an argument that starts with `-` as an option. Feed SQL
  on stdin.
- The e2e telemetry golden shows `-0% of spend while idle`. That is a
  negative-zero render in the insights line. It is frozen as is; a later
  fix must re-bless `telemetry-e2e.txt` in its own commit.
- `CURRENT_PHASE` holds only a comment line. The orchestrator appends
  phases; `just gate` then also checks those phases' IDs.
