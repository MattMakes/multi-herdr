# U05 a1-vocabulary report (phase A1)

Branch: `ard/a1-vocabulary`. Requirements: ARC-02, ARC-03, ARC-04.

## New public types

- `horch_core::ids` (`crates/horch-core/src/ids.rs`):
  - `string_id!` newtypes: `ExecutionId`, `TaskId`, `SessionId`, `WorkerId`,
    `RoleName`, `SkillId`, `ModelId`, `TeammateName`, `PaneId`,
    `WorkspaceId`, `ExperimentId`, `RoundId`, `EventId`, `JudgmentId`.
  - Each has `new()`, `as_str()`, `Display`, `AsRef<str>`, `FromStr`,
    `TryFrom<String>`, `From<Id> for String`, and serde that validates on read.
  - `IdError` (hand-written `Display` + `Error`).
  - `mint_v7(at) -> uuid::Uuid` and `mint(at)` on `ExecutionId`, `EventId`,
    `ExperimentId`, `RoundId`, `JudgmentId`.
  - `WorkerId::from_parts(&WorkspaceId, &RoleName)`.
- `horch_core::harness::HarnessKind` (`crates/horch-core/src/harness/mod.rs`).
  `teammates::Agent` is now `pub use crate::harness::HarnessKind as Agent`.
- `horch_core::execution::model` (re-exported from `horch_core::execution`):
  `ExecutionStatus`, `FailureKind`, `LaunchStage`, `ExecutionKind`,
  `SessionMode`, `SessionState`, `TilingMode`.

## Call sites changed

- `Agent::Opencode` became `Agent::OpenCode` in `balance_policy.rs`,
  `launch.rs`, `skills.rs` and `crates/horch/src/cmd/worker.rs`.
- `prompts::worker_prompt` takes `session: &SessionMode` (was `resume: bool`).
  Callers: `prompts.rs` tests, `tests/golden_prompts.rs`, `cmd/worker.rs`.
- `mailbox::Brief`: `session_id: String` + `resume: bool` became
  `session: SessionMode`. Callers: `cmd/spawn.rs`, `cmd/recipes.rs`,
  `cmd/worker.rs`, `mailbox.rs` tests.
- `cmd/spawn.rs`: `SpawnArgs.no_tile` became `tiling: TilingMode`. The private
  `Plan` holds `session: SessionMode` instead of `session_id` + `resume`.
- `main.rs`: the clap field is `untiled` with `#[arg(long = "no-tile")]`.
  It converts with `TilingMode::from_no_tile` at the CLI boundary.
- `cmd/smoke.rs`: 2 `SpawnArgs` literals use `TilingMode::Automatic`.

## Decisions

- Serde for ids uses `try_from = "String", into = "String"`.
  `serde(transparent)` cannot be combined with `try_from`. The JSON is the
  same bare string.
- The plan asks for `WorkerId::new(&WorkspaceId, &RoleName)`. `new()` is the
  validating string constructor that every id shares, so the two-part
  constructor is `WorkerId::from_parts`.
- `WorkerId` validates the role part with the `RoleName` rules.
- `ExecutionStatus` serializes through a private `StatusWire` enum:
  `{"state":"failed","failure":{"kind":"agent_exited","code":1}}`,
  `{"state":"launch_failed","stage":"split","reason":"..."}`. Serde cannot
  tag a newtype variant that wraps an enum.
- `ExecutionKind` serializes with a `kind` tag, snake_case.
- `Brief` keeps its on-disk shape (`session_id` string, empty until known,
  and `resume` bool) through `#[serde(flatten, with = "session_wire")]`.
  Test `arc_04_brief_session_wire_unchanged` proves this. A brief with
  `resume: true` and an empty `session_id` is now a parse error. `horch spawn`
  never writes one.
- The ledger `Record.session_id` stays `Some("")` for an agent that mints its
  own id, as before.
- `ledger::Record` is unchanged (A6 adds `state`).

## Gotchas for A2 and later

- `SessionMode::Fresh(None)` means "the agent mints its id". Launch maps
  `Fresh(_)` on a non-minting agent to `launch::Session::Unmanaged`.
- `launch::Session` (borrowed `&str`) still exists next to `SessionMode`.
  A4 can fold them together.
- `mint_v7` clamps a pre-1970 instant to 0 ms.
- No existing struct adopts the newtypes yet, except `Brief.session`.

## Gate

- `just gate` does not exist in the base. I ran the 7 manual commands.
- fmt, build, `teammates --check` and `check-deps` pass.
- `cargo test --workspace --no-fail-fast` had 47 failures on base 9587e24.
  The failure set on this branch is equal to that set. The orchestrator set
  this rule until the baseline fix merges.
- `golden_prompts` passes (6 of 6). No golden file changed.
