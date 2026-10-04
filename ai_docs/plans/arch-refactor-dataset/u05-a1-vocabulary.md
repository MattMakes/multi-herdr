# U05 a1-vocabulary: identity newtypes, HarnessKind, execution model types

Unit slug: `a1-vocabulary`. Branch: `ard/a1-vocabulary`. Phase: A1.
Requirements: ARC-02, ARC-03, ARC-04.

## GOAL

The core has typed identities (`ids.rs`), `harness::HarnessKind` replaces
`teammates::Agent` (with a shim), `execution::model` holds the typed status,
kind and mode enums, and `no_tile`/`resume: bool` parameters become
`TilingMode`/`SessionMode`. No user-visible behavior changes.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Operator decisions" (OD7), "Hard
  constraints", §1 concept map, §2 "Key types" (ids, ExecutionStatus,
  ExecutionKind, SessionMode, TilingMode, Execution, LedgerRecordV1,
  HarnessKind), §3 "A1", §4 ARC-02..04 rows.
- Today: `crates/horch-core/src/teammates.rs:203` defines
  `pub enum Agent { Claude, Codex, Opencode, Pi, Prime, None }` with
  `as_str()`. `crates/horch-core/src/lib.rs` has `mint_uuid()` (v4).
  `crates/horch-core/src/ledger.rs:52` defines `Record` with `status: String`
  (`"working"` or `"done"`).
- This is the critical path. Phases A2 to A7 build on your types. Keep the
  change mechanical and the diff reviewable.
- Parallel units: U02 writes oracle tests that call `launch::command` and
  `balance_policy::decide` on the old signatures. U06 adds `fsx.rs`,
  `measure/` and `usage/money.rs`; U06 owns the `Digest` type in
  `measure/digest.rs`, so do NOT define `Digest` in `ids.rs`.
  U08 adds `runtime/machine.rs` and `teacher/`. Expect `lib.rs` merge
  conflicts; keep both sides.

## FILES

own:
- `crates/horch-core/src/ids.rs` (new)
- `crates/horch-core/src/harness/mod.rs` (new; `HarnessKind` only in this phase)
- `crates/horch-core/src/execution/mod.rs` (new)
- `crates/horch-core/src/execution/model.rs` (new)
- `crates/horch-core/src/lib.rs` (add `pub mod execution; pub mod harness; pub mod ids;`)
- `crates/horch-core/src/teammates.rs` (replace the `Agent` definition with the shim)
- the call sites of `no_tile` and `resume: bool` parameters, and of
  `Agent::Opencode` if you rename the variant. Find them with
  `grep -rn 'no_tile\|resume: bool\|Agent::' crates`. These are in
  `crates/horch-core/src/` and `crates/horch/src/`. Change only the lines
  needed for the type change.
- `ai_docs/reports/arch-refactor-dataset/a1-vocabulary.md`

do not touch: `crates/horch-core/tests/golden/**`, oracle files, `scripts/`,
`ai_docs/designs/`, `crates/horch-marketplace/`, `crates/horch-e2e/src/bin/`.

## STEPS

1. Create the worktree (conventions §2). Run `cargo test --workspace` once to
   warm the build.
2. `ids.rs`:
   - A `string_id!` macro that defines a newtype over `String` with
     `Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash`,
     `serde(transparent)`, `Display`, `AsRef<str>`, `as_str()`, and
     `TryFrom<String>` / `FromStr` / `new()` that run a per-type validator.
     Deserialization must validate too (use `serde(try_from = "String")`).
   - Newtypes: `ExecutionId`, `TaskId`, `SessionId`, `WorkerId`, `RoleName`,
     `SkillId`, `ModelId`, `TeammateName`, `PaneId`, `WorkspaceId`,
     `ExperimentId`, `RoundId`, `EventId`, `JudgmentId`.
   - Validators: every id is non-empty and has no control characters.
     `RoleName` also rejects `/`, `\` and `..`. `WorkerId` is
     `"<workspace>:<role>"` (exactly one `:` separating two non-empty parts;
     add `WorkerId::new(&WorkspaceId, &RoleName)`). `ExecutionId` accepts any
     UUID and also legacy non-UUID ids such as `rec-o1` and `perf-3` (OD7).
   - `pub fn mint_v7(at: chrono::DateTime<chrono::Utc>) -> uuid::Uuid`: take
     `Uuid::new_v4()` bytes, write the 48-bit big-endian Unix millisecond
     timestamp into bytes 0..6, set the version nibble to 7 (byte 6 high
     nibble), keep the RFC 4122 variant bits. Do not change the uuid crate
     features. Add `ExecutionId::mint(at)` and the same for `EventId`,
     `ExperimentId`, `RoundId`, `JudgmentId`.
   - Tests in `ids.rs`: `arc_02_ids_validate`, `arc_02_mint_v7_layout`
     (version nibble 7, variant bits 10, timestamp round-trips, two mints in
     the same ms differ, ordering by time), `arc_02_legacy_ids_accepted`
     (`rec-o1`, `perf-3`, a v4 UUID).
3. `harness/mod.rs`:
   - `pub enum HarnessKind { Claude, Codex, OpenCode, Pi, Prime, None }` with
     the same serde spelling as today's `Agent` on disk and in teammate
     frontmatter (`claude`, `codex`, `opencode`, `pi`, `prime`, `none`). Use
     `#[serde(rename = "opencode")]` on `OpenCode`. Keep `as_str()` and every
     other method and trait impl that `Agent` has today, unchanged.
   - In `teammates.rs`, delete the `Agent` enum and its impl, and add
     `pub use crate::harness::HarnessKind as Agent;`. Existing code that
     writes `Agent::Opencode` must change to `Agent::OpenCode` (the alias
     shares the variants). Update those call sites.
   - Test `arc_03_harness_kind_serde_compat`: every variant round-trips
     through JSON and YAML with the old strings; a teammate frontmatter with
     `agent: opencode` parses; `Agent` and `HarnessKind` are the same type.
4. `execution/model.rs`:
   - `ExecutionStatus { Planned, Starting, Running, Done, Failed(FailureKind), LaunchFailed { stage: LaunchStage, reason: String } }`,
     `FailureKind { AgentExited { code: Option<i32> }, PaneVanished, TimedOut, Cancelled { reason: String }, Crashed }`,
     `LaunchStage { Brief, Split, Run }`.
     `is_live()` is true for `Starting` and `Running` only.
     `is_terminal()` is true for `Done`, `Failed`, `LaunchFailed`.
   - Legacy mapping: `legacy_status(&self) -> &'static str` returns
     `"working"` for Planned/Starting/Running and `"done"` for every terminal
     state. `from_legacy(status: &str) -> ExecutionStatus` maps `"working"` to
     `Running` and `"done"` (and any other value) to `Done`.
     `resolve(state: Option<&ExecutionStatus>, status: &str)` prefers `state`.
     Serde for `ExecutionStatus`: internally tagged, snake_case
     (`{"state":"failed","failure":{"kind":"agent_exited","code":1}}` or a
     similar flat shape; document the shape in a doc comment).
   - `ExecutionKind { Worker, Orchestrator, Candidate { experiment: ExperimentId, round: RoundId, label: String }, Judge { round: RoundId, attempt: u32 } }`
     with `legacy_kind()` returning `"worker"` or `"orchestrator"`
     (Candidate and Judge map to `"worker"`).
   - `SessionMode { Fresh(Option<SessionId>), Resume(SessionId) }`,
     `SessionState { Pending, Known(SessionId), Unavailable }`,
     `TilingMode { Automatic, Disabled }` with
     `TilingMode::from_no_tile(bool)`.
   - Do NOT change `ledger::Record` in this phase. The new optional `state`
     field and the other new record fields come in A6.
   - Tests: `arc_04_status_legacy_roundtrip` (every status maps to the right
     legacy string; `resolve` prefers `state`; LaunchFailed and Failed map to
     `"done"`, never `"working"`), `arc_04_tiling_mode`.
5. Replace bool parameters:
   - Every function parameter named `no_tile: bool` becomes `tiling: TilingMode`.
   - Every `resume: bool` parameter becomes `session: SessionMode` where a
     session id is in scope next to it, or a two-variant enum where it is
     not. Do not change CLI flags: clap still parses `--no-tile` and
     `--resume`; convert at the CLI boundary.
   - Check: `grep -rn 'no_tile: bool\|resume: bool' crates` prints nothing.
6. Run the gate after each step. Commit per step:
   `A1: Add identity newtypes`, `A1: Introduce HarnessKind`,
   `A1: Add execution model types`, `A1: Replace bool modes with types`.
7. Verify no user-visible change: `cargo test --workspace` passes, including
   `golden_prompts`; `horch teammates --check` passes. If U02's oracle tests
   are on the integration branch when you rebase, they must pass unchanged in
   data; adapt only their call sites.
8. Write and commit the report. Follow conventions §6 to finish.

## CONSTRAINTS

- Review rule for this phase: no new raw `String` id fields or parameters in
  the code you write.
- Do not convert existing structs to the newtypes in this phase except where
  a step says so. Later phases adopt them.
- `chrono` and `uuid` are already dependencies. Add no crate.

## DONE WHEN

- Tests `arc_02_ids_validate`, `arc_02_mint_v7_layout`,
  `arc_02_legacy_ids_accepted`, `arc_03_harness_kind_serde_compat`,
  `arc_04_status_legacy_roundtrip`, `arc_04_tiling_mode` pass.
- `grep -rn 'no_tile: bool\|resume: bool' crates` prints nothing.
- `grep -n 'pub enum Agent' crates/horch-core/src/teammates.rs` prints nothing.
- The full gate is green and golden prompts are unchanged.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the new public types and their paths, every call site
  changed, and gotchas for A2.
