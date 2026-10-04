# D16 dead-code: every public item with no caller is deleted, wired or documented

Branch `ds/dead-code`. Commits:

- `Core: Delete public items that have no caller`
- `Messaging: Write spawn briefs through Mailbox::write_brief`
- `Workspace: Delete unused herdr calls and test helpers`
- `Docs: Name the design seam for the teacher types and replace_durable`
- `VCS: Delete GitClient::merge_ff_only`
- `Tests: Drop the stale brief edit in skl_06_e2e_marketplace_offline`
- `Reports: Add the D16 dead-code report`

## Method

For each item from the A12 list ("Kept `pub` although nothing uses them") and
the D13 addition, I grepped every crate, tests included. The rule from the
plan: delete, unless a test or a documented seam needs it; wire it where a
caller is missing.

## Decisions

| Item | Decision | Reason |
|---|---|---|
| `execution::store::ExecutionStore::find_by_idempotency` | deleted | The coordinator already has the idempotency path. `Coordinator::records()` reads the store once per tick and indexes it by `Execution::idempotency_key()`. `next_launch` skips a label whose `spawn:<round>:<label>` key is in that index, and `observe` adopts a record that a killed coordinator left. `cmp_13_crash_every_boundary` (`abort-after-candidate-spawned:1`) proves it: after `resume`, the store has 2 candidate records and the log has 2 `candidate.spawned`. `b3-coordinator.md` recorded the same decision. |
| `execution::records::Ledger::set_routing` | deleted | No caller. `execution::plan` sets `routing` on the record before the insert. |
| `competition::judging::DEFAULT_JUDGE_TIMEOUT` | deleted | No caller, and it was wrong: 20 min, while the real default is `competition::config::DEFAULT_JUDGE_TIMEOUT_S` = 900 s. |
| `competition::model::{ValidationRun, JudgmentRef}` | deleted | No caller. No design names `JudgmentRef`. Validation results travel on the `validation.completed` event (`ValidationReport`) and the judgment is `evaluation::judgment::Judgment`. The unused `JudgmentId` import went with them. |
| `harness::HarnessKind::{takes_tool_lists, takes_tool_denylist}` | deleted | Wrappers of `Capabilities::{tool_lists, tool_denylist}`. Only `arc_10_capabilities_match_legacy_predicates` called them, and that test asserts the same fields directly 2 lines above. I removed the 2 duplicate asserts. |
| `measure::testkit::SplitMix64::next_f64` | deleted | Only its own range check in `below_stays_in_range` called it. I removed that loop. |
| `horch::dataset::exit::NOT_IMPLEMENTED` | deleted | Every dataset command is implemented. No command returns 2. Clap still uses 2 for usage errors. |
| `messaging::mailbox::Mailbox::write_brief` | wired | `ExecutionService::write_brief` wrote the brief file itself, next to the mailbox's own (non-atomic) writer. The mailbox writer is now atomic with mode 0644 (`BRIEF_MODE` moved to `mailbox.rs`), and the service calls it. Same path, same bytes, same mode. |
| `workspace::client::HerdrClient` | deleted | Type alias with no caller. I also removed the stale module doc sentence "No caller uses the trait yet; A7 part b moves them over" (the service uses `WorkspaceClient`). |
| `workspace::herdr::Herdr::{focused_pane, tab_create, tab_close}` | deleted | No caller. `tile` uses `pane_move_new_tab`, and herdr closes an empty tab itself. `TabCreateResult` went with `tab_create`. |
| `workspace::model::NewTab` | deleted | Only `tab_create` returned it. `tile::Op::NewTab` is a different item and stays. |
| `workspace::layout::analyze` | kept, `#[cfg(test)] pub(crate)` | 18 layout tests call it. The crate calls `analyze_with`. The `balance.rs` doc link now names `analyze_with` as a code span. |
| `workspace::testing::FakeWorkspace::workspace_ids` | deleted | No caller. |
| `teacher::{DecisionModel, inert::Inert, system_one::{DecisionRequest, DecisionResponse}}` | kept, documented | The OD4 seam. Each doc comment now points at `ai_docs/designs/2026-10-02-dataset-competition-design.md` §1.3. |
| `fsx::replace_durable` | kept, documented | The design names it: §2.3 (MEA-09) and the test map row `mea_09_replace_durable`. The doc comment now says so. Wiring it into the projection, manifest and export writers is a follow-up (see below). |
| `vcs::git::GitClient::merge_ff_only` (D13) | deleted | No caller in `src/` since promotion uses `read_tree_update`. Removed from the trait, `SystemGit`, the 4 test fakes (`judge.rs` in e2e, `judging.rs`, `judge_input.rs`, `promotion.rs`) and the 4 lines in `vcs.rs` that exercised it. The receipt still calls the publish mode `merge_ff_only`; that is a string in the data, not the method. |

## The skl_06 workaround

`skl_06_e2e_marketplace_offline` wrote a teammate without `demo`, then
edited the brief after spawn to add `demo`. `horch spawn` now checks names with
`ensure_supported_in` against the installed catalog, so the test keeps the
teammate with `["demo", "tdd"]` and launches it directly. It still sets
`HORCH_GIT_BIN=/nonexistent` before the launch and asserts that `demo` loads,
so it still proves offline use.

## Gate

- `cargo build --workspace --all-targets`: 0 warnings.
- `cargo doc --workspace --no-deps`: 0 warnings in `horch-core` and `horch`.
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: green after the rebase (see the merge line).

## Gotchas

- The first gate run had 1 failure: `cmp_05_e2e_candidates_in_dataset_workspace`
  saw 1 `candidate.completed` instead of 2. The test passed 3 of 3 alone and
  the full `dataset` suite passed 2 of 2 after. It is a timing flake under
  load, not caused by this unit.

## Follow-ups (outside my scope)

- `fsx::replace_durable`: the design says round projections, the manifest and
  export files use it. The code calls `write_atomic` (same semantics). A small
  unit can switch those callers, or the design can name `write_atomic`.
- `ai_docs/reports/arch-refactor-dataset/a6b-service.md` still says "B3 calls
  `find_by_idempotency` before it plans". That is history; B3's report records
  the change.
- `cargo doc` warns in `herdr-install/src/main.rs:3` and
  `herdr-docs-sync/src/main.rs:3` (bare URLs). These are pre-existing.
