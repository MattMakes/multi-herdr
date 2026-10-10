# Recipe: add a harness

## When

You want horch to launch a new agent CLI in a worker pane, next to `claude`,
`codex`, `opencode`, `pi`, `prime` and `antigravity`.

## Before you start

- [architecture.md](../architecture.md), the `harness` row and the arch-scan
  rules (`arc_10_harness_match_only_in_harness` matters most here).
- Find out from the CLI's own docs: how it takes a prompt, model and effort;
  who mints the session id and where it is stored; how to resume; its
  permission modes; which env vars move it off the operator's login.

In the steps, `<name>` is the harness name (`antigravity`), `<Name>` the
variant (`Antigravity`) and `<bin>` the binary (`agy`).

## Steps

1. Write the adapter `crates/horch-core/src/harness/<name>.rs`: a unit struct
   that implements the `Harness` trait (`harness/mod.rs`, `pub trait Harness`).
   It builds argv for a fresh and a resumed launch, discovers the session id
   when the CLI mints it, maps permission modes, and removes the CLI's own
   forbidden env vars with `env_remove`. Put unit tests in the same file.
   Check: `cargo test -p horch-core harness::<name>`.
2. In `crates/horch-core/src/harness/mod.rs`: add `pub mod <name>;`, the
   `HarnessKind::<Name>` variant, the entry in `HarnessKind::ALL`, and an arm
   in `as_str`, `binary`, `adapter`, `capabilities` and `FromStr`. In the
   tests module add the kind to the local `ALL` array and to the `legacy`
   table. Check: `all_names_every_kind` and
   `arc_10_capabilities_match_legacy_predicates` pass.
3. In `crates/horch-core/src/harness/capabilities.rs`: add a
   `pub(crate) const <NAME>: Capabilities`. Set `skill_exposure` to
   `SkillExposure::None` unless the CLI can be pointed at a private skills
   directory. Check: `cargo build -p horch-core`.
4. In `crates/horch-core/src/harness/launch.rs` tests: add the kind to the
   `unreachable!()` arm of the skill-exposure test when it has no skill
   exposure. Check: `cargo test -p horch-core harness::launch`.
5. In `crates/horch-core/src/roster/permission.rs`: add a `<name>_args`
   method that maps each `PermissionMode` to flags, `None` for a refused
   mode. In `crates/horch-core/src/roster/validation.rs`: add the
   `HarnessKind::<Name>` arm next to `mode.antigravity_args()`, and a rule
   that refuses a teammate `env` that sets a forbidden key. Check:
   `an_antigravity_teammate_cannot_set_a_google_key` is the model test; add
   the same for your harness.
6. In `crates/horch-core/src/routing/quota.rs`: add a `pool_for` arm. Add a
   `POOL_*` const and an entry in `POOLS` only for a new pool. Check: the
   `pool_for` assertions in the quota tests.
7. In `crates/horch-core/src/runtime/bins.rs`: add the `HORCH_<NAME>_BIN`
   override (the field in `BinOverrides`, the name in its list, the match
   arm, the field in `HarnessBins`, a `<name>_bin` default). In
   `crates/horch-core/src/runtime/context.rs` raise the count in
   `arc_05_context_from_map_env` (`env_pairs().len()`) by 1. Check:
   `cargo test -p horch-core runtime::`.
8. Nothing to edit for `horch agent-list` (`harness/inventory.rs` iterates
   `HarnessKind::ALL`) or for dataset preflight (`kind_of` in
   `crates/horch/src/dataset/preflight.rs` iterates `HarnessKind::ALL`).
   Check: `./target/debug/horch agent-list --no-probe` shows a row for
   `<name>`; `kind_of_names_every_harness` passes.
9. Write the fake `crates/horch-e2e/src/bin/fake-<name>.rs`, add a `[[bin]]`
   entry to `crates/horch-e2e/Cargo.toml`, and add `("<bin>", "fake-<name>")`
   to `FAKES` in `crates/horch-e2e/src/harness.rs` (raise the array length).
   Check: `cargo build --workspace --bins`.
10. Add e2e tests: `arc_26_e2e_lifecycle_matrix_<name>` in
    `crates/horch-e2e/tests/lifecycle.rs` (spawn, session found, `horch
    done`, `spawn --resume` reuses the id) and `skl_06_e2e_exposure_<name>`
    in `crates/horch-e2e/tests/skills_exposure.rs`. Add a test that the
    child never receives a forbidden key (model:
    `antigravity_child_never_receives_a_forbidden_key`). Check:
    `cargo build --workspace --bins && cargo test -p horch-e2e --test lifecycle`.
11. Add the teammate `teammates/<name>.md` (see
    [add-teammate.md](add-teammate.md)). With `SkillExposure::None`, give it
    no `phase` and no `skills`, and add its name to the no-phase arm of
    `builtin_phase_defaults_are_portable_and_disabling_conflicts` in
    `roster/validation.rs`. Add it to `SKIP_NEW_TEAMMATES` in
    `crates/horch-core/tests/baseline_oracles.rs` and
    `crates/horch-core/tests/skills_catalog.rs`. Check:
    `./target/debug/horch teammates --check`.
12. Docs: the `agent:` list in `teammates/_template.md`, the trust table in
    `teammates/README.md` if its provider trains on input, the harness table
    and the count heading ("Six harnesses") and the environment table in
    `README.md`, the module table in `crates/horch-core/src/lib.rs`, and
    [command-flow.md](../command-flow.md). Check: read them once.
13. Optional: `horch cost` support. Add a reader to `telemetry/readers.rs`
    for tokens and a skill scan to `usage.rs` (the `match agent` in
    `skill_loads_in`), only if the CLI writes a local usage or transcript
    file. Antigravity has none.
14. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Files this recipe touches

| file | change | required or optional |
|---|---|---|
| `crates/horch-core/src/harness/<name>.rs` | new adapter and unit tests | required |
| `crates/horch-core/src/harness/mod.rs` | module, variant, `ALL`, 5 match arms, test tables | required |
| `crates/horch-core/src/harness/capabilities.rs` | `Capabilities` const | required |
| `crates/horch-core/src/harness/launch.rs` | test match arm | required when no skill exposure |
| `crates/horch-core/src/roster/permission.rs` | permission mapping | required |
| `crates/horch-core/src/roster/validation.rs` | permission arm, env rule, phase-defaults test | required |
| `crates/horch-core/src/routing/quota.rs` | `pool_for` arm; `POOLS` for a new pool | required |
| `crates/horch-core/src/runtime/bins.rs` | `HORCH_<NAME>_BIN` | required |
| `crates/horch-core/src/runtime/context.rs` | override count in a test | required |
| `crates/horch-e2e/src/bin/fake-<name>.rs` | the fake | required |
| `crates/horch-e2e/Cargo.toml` | `[[bin]]` | required |
| `crates/horch-e2e/src/harness.rs` | `FAKES` entry | required |
| `crates/horch-e2e/tests/lifecycle.rs` | lifecycle and env tests | required |
| `crates/horch-e2e/tests/skills_exposure.rs` | exposure test | required |
| `teammates/<name>.md` | the teammate | required |
| `crates/horch-core/tests/baseline_oracles.rs`, `skills_catalog.rs` | `SKIP_NEW_TEAMMATES` | required |
| `teammates/_template.md`, `teammates/README.md`, `README.md`, `crates/horch-core/src/lib.rs` | docs | required |
| `crates/horch-core/src/usage.rs`, `telemetry/readers.rs` | cost reader | optional |

## Tests and oracles

- Must exist: the adapter's unit tests, `arc_26_e2e_lifecycle_matrix_<name>`,
  `skl_06_e2e_exposure_<name>`, a forbidden-env e2e test, a `pool_for`
  assertion, a validation test for the env rule.
- Must still pass unchanged: `arc_10_harness_match_only_in_harness`,
  `arc_05_no_ambient_env_in_core`, `all_names_every_kind`,
  `kind_of_names_every_harness`.
- Oracles and goldens: none change. A new teammate gets no oracle file (see
  [testing-and-gates.md](../testing-and-gates.md#horch_bless)).

## Gotchas

- Forbidden env: the global `FORBIDDEN_ENV` in `harness/launch.rs` holds only
  `ANTHROPIC_API_KEY` and applies to every child process. Put a key that
  only your CLI must not see in a per-adapter list (model:
  `ANTIGRAVITY_FORBIDDEN_ENV` in `harness/antigravity.rs`), because other
  tools of a worker can need it.
- A harness with `SkillExposure::None` must not get a `phase`:
  `skills::selection::check` refuses selected skills for it, and the spawn
  fails.
- You verify against the fake only. List every flag, path and file format
  that you did not run against the real CLI in your report, and give the
  operator a local acceptance checklist (model: the end of
  `antigravity-harness.md`).

## Worked example

`git show --stat 81e6b33` (D08, the Antigravity harness) shows 21 files.
`git show --stat 4b62d9e` (D10) added `agy` to `FAKES`, made `kind_of`
iterate `HarnessKind::ALL`, and added `google` to `POOLS`.
