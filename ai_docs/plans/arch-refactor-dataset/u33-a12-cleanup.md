# U33 a12-cleanup: delete the shims, narrow visibility, update docs

Unit slug: `a12-cleanup`. Branch: `ard/a12-cleanup`. Phase: A12.
Requirements: ARC-25 (and every earlier test stays green).

## GOAL

The refactor ends with no re-export shim and no `ledger.rs` facade, every
caller importing from the module that owns the item, `pub(crate)` as the
default visibility with `#![warn(unreachable_pub)]` clean, and the lib.rs
module table, README architecture section and design-doc statuses matching
the code.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "A12", §4 ARC-25 row.
- This unit runs ALONE after every other unit has merged. It touches many
  files; no other worker is active. You own the whole tree for this unit,
  except the frozen data below.
- Shims known from the merged reports (`ai_docs/reports/arch-refactor-dataset/*.md`;
  grep them for "shim", "re-export", "A12"; also `STATUS.md` "Gotchas"):
  `ledger.rs` facade and `ledger::state_root`, `teammates.rs` (roster
  wrappers), `mailbox.rs`, `message.rs`, `Herdr::send_line`,
  `tilecmd::after_change`, `balancecmd::equalize_quietly`, the `ProcessEnv`
  shims in `roster/validation.rs`, `Bundle::configure` and `Bundle::apply_env`
  (delegates kept for `baseline_oracles.rs` and `command_with_skills_in`),
  `skills::ensure_supported` (replaced by `ensure_supported_in`), `telemetry/readers.rs`, `prime.rs`
  (`Daemon::finish` uses `agent::prime_bin`), and any `PENDING` lists in
  `crates/horch-core/tests/arch_scan.rs`. Find the rest with
  `grep -rn 'pub use' crates/*/src` and by reading each top-level `*.rs`
  file in `crates/horch-core/src` that only re-exports.
- Keep (on-disk formats still exist): `LedgerRecordV1`, the Brief v1
  defaults, the `tier` key, legacy `status` words.

## FILES

own: every Rust source file in `crates/**`, `README.md`,
`crates/horch-core/src/lib.rs`, `ai_docs/designs/2026-10-02-*.md` (status
lines only), `ai_docs/reports/arch-refactor-dataset/a12-cleanup.md`.

do not touch: oracle and golden data, `teammates/**` prompt text,
`scripts/**` except to remove an `A12` pending marker.

## STEPS

1. Create the worktree (conventions §2).
2. List every shim (a module whose items only re-export another module's
   items, a wrapper kept "until A12", a `PENDING` entry). Write the list in
   the report first.
3. For each shim: move every caller to the owning module, delete the shim,
   build, test. 1 commit per shim group.
4. Empty every `PENDING` list in `arch_scan.rs`; the scans must pass with
   no exception.
5. Add `#![warn(unreachable_pub)]` to `horch-core` and `horch` libs; change
   `pub` to `pub(crate)` where nothing outside the crate uses the item
   (the binaries, `horch-e2e` and integration tests are outside users).
   `cargo build` with `-D warnings` for that lint must be clean.
6. Test `arc_25_no_shim_modules` (in `arch_scan.rs`): no source file
   consists only of `pub use`/`mod` lines; no `ledger.rs`; no item named in
   the shim list exists.
7. Docs: the lib.rs module table, README architecture section, and
   `Status:` lines of both designs (`Implemented`, with the phase list).
8. Run the full gate and every e2e with `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- `grep -rn 'A12' crates/` finds no pending marker.
- `check-req-coverage.sh --phase A12` exits 0; `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the shims removed, visibility changes, anything kept and why.
