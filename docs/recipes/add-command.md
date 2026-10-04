# Recipe: add a CLI command

## When

You want a new `horch <command>` (or a new `multi-herdr-dataset <command>`).

## Before you start

- [architecture.md](../architecture.md): the CLI layer and the arch-scan
  rules.
- `ai_docs/reports/design-skills/agent-list.md`: the last command added.

In the steps, `<name>` is the command, for example `agent-list`, and
`<file>` its module name, for example `agentlist`.

## Steps for `horch`

1. Put the logic in the `horch-core` module that owns the concern, as a pure
   function where you can: the CLI gathers facts (paths, a probe, a cached
   file) and core turns them into a result. D09 put the inventory in
   `crates/horch-core/src/harness/inventory.rs` and registered it with
   `pub mod inventory;` in `harness/mod.rs`. Add unit tests there. Check:
   `cargo test -p horch-core <module>::`.
2. Add `crates/horch/src/cmd/<file>.rs` with a `pub fn` that takes
   `&RuntimeContext` (or `&mut`) and the flags, calls core and prints. Use
   `crate::output::println` / `output::print` for bulk stdout. Load the
   roster with `super::load_roster(ctx, None)` when you need it. Add
   `pub mod <file>;` to `crates/horch/src/cmd/mod.rs` (alphabetical). Check:
   `cargo build -p horch`.
3. In `crates/horch/src/main.rs`: add a variant to `enum Command` with doc
   comments. The doc comments are the `--help` text; clap derives the
   kebab-case name (`AgentList` becomes `agent-list`). Add the dispatch arm
   in `run()` (model: `Command::AgentList { json, no_probe } =>
   cmd::agentlist::agent_list(ctx, json, no_probe)?`). Check:
   `cli_definition_is_valid` passes and `./target/debug/horch <name> --help`
   prints your text.
4. Errors and exit codes: return `anyhow::Result`. For a non-1 exit code, add
   a const to `crates/horch/src/exit.rs` and pick it from a typed error
   (`downcast_ref`), never from the message text. Check:
   `arc_22_no_error_string_matching`.
5. Read no environment variable in the command or in core. Add the variable
   to `RuntimeContext` in `crates/horch-core/src/runtime/` and read it from
   `ctx`. Check: `arc_05_no_ambient_env_in_core`,
   `arc_06_env_mutation_sites_reduced`.
6. Add `crates/horch/tests/<file>.rs`. It runs the built binary
   (`Command::new(env!("CARGO_BIN_EXE_horch"))`) with `env_clear()`, a temp
   `HOME`, `XDG_DATA_HOME` and `HORCH_STATE_DIR`, and a fake script through
   `HORCH_<NAME>_BIN` when the command runs an agent CLI.
   Model: `crates/horch/tests/agent_list.rs`. Check:
   `cargo test -p horch --test <file>`.
7. Docs: `README.md` (the command table or the section it belongs to) and
   [command-flow.md](../command-flow.md) (a diagram node and a row in
   "Together or apart"). If agents should run the command, also
   `skills/orchestrate/SKILL.md` and `teammates/_base/*.md`; a change to
   `_base` needs a sanctioned block in
   `crates/horch-core/tests/golden_prompts.rs`.
8. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Steps for `multi-herdr-dataset`

1. Core logic in `crates/horch-core/src/competition/`, `measure/`,
   `evaluation/` or `dataset/`.
2. Add the subcommand to `enum Command` in `crates/horch/src/dataset/cli.rs`
   (`#[command(hide = true)]` for an operator-only or internal command), a
   module `crates/horch/src/dataset/<file>.rs` returning `Result<u8>` (the
   exit code), `pub(crate) mod <file>;` and a dispatch arm in
   `crates/horch/src/dataset/mod.rs`. Exit codes are in `dataset::exit` in
   the same file.
3. Tests in `crates/horch/tests/dataset_cli.rs`: `cmp_01_cli_args` parses
   flags with `Cli::try_parse_from`; the `BIN` const runs the built binary.
   Check: `cargo test -p horch --test dataset_cli`.
4. Write every state change as an event; see
   [add-dataset-event.md](add-dataset-event.md).

## Files this recipe touches

| file | change | required or optional |
|---|---|---|
| `crates/horch-core/src/<module>/<file>.rs` | domain logic and unit tests | required when there is logic |
| `crates/horch-core/src/<module>/mod.rs` | `pub mod` line | with a new core file |
| `crates/horch/src/cmd/<file>.rs` | the command | required |
| `crates/horch/src/cmd/mod.rs` | `pub mod` line | required |
| `crates/horch/src/main.rs` | `Command` variant and dispatch arm | required |
| `crates/horch/src/exit.rs` | exit code const | optional |
| `crates/horch/tests/<file>.rs` | CLI test | required |
| `README.md`, `docs/command-flow.md` | docs | required |
| `skills/orchestrate/SKILL.md`, `teammates/_base/*.md`, `crates/horch-core/tests/golden_prompts.rs` | agent-facing docs | optional |

## Tests and oracles

- Must exist: unit tests for the core logic; a CLI test that runs the
  built binary.
- Must still pass: `cli_definition_is_valid` (in `main.rs`),
  `arc_05_no_ambient_env_in_core`, `arc_22_no_error_string_matching`,
  `arc_25_no_shim_modules`.
- Oracles: a new command changes none. A change to the output of `horch
  skills` or `horch sessions` changes `crates/horch/tests/oracles/`; that
  needs a plan that says so.

## Gotchas

- A command that runs an agent CLI must never start a model turn by accident.
  `agent-list` runs only `--version`, and `--no-probe` runs nothing.
- Probe timeouts: macOS scans a new executable on its first run, which can
  take seconds under load. Use the shared probe (`harness_version` in
  `routing/quota_probe.rs`) rather than a short timeout of your own.
- `horch` subcommands that need herdr build a `Herdr` client from
  `ctx.bins.harness.herdr`. A command that does not need herdr must not build
  one, so it works without a server.

## Worked example

`git show --stat b0c241b` (D09, `horch agent-list`): 8 files.
`harness/inventory.rs` holds the pure logic, `cmd/agentlist.rs` gathers the
facts, `main.rs` and `cmd/mod.rs` gain 12 lines, `tests/agent_list.rs` runs
the binary against fake binaries.
