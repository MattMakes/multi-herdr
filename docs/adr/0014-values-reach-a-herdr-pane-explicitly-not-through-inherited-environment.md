# Values reach a herdr pane explicitly, never through the environment

A herdr pane is a fresh shell started by the herdr server. It carries the
server's (the operator's) environment and user profile, not the
environment of the process that spawned it. So every value a pane needs
from the spawner travels explicitly: on the pane command line, in the
worker's brief (`crates/horch-core/src/messaging/brief.rs`, `Brief`), or as
an explicit argument. Without that, the worker silently falls back to
defaults. For example, `HORCH_CLAUDE_BIN=claude horch fleet` would have no
effect on the workers it spawns, and a worker would read the compiled-in
roster copies and ignore local edits.

## Consequences

- `ExecutionService::run` (`crates/horch-core/src/execution/service.rs`)
  runs `horch worker <role>` in the new pane with this process's skill
  store as `HORCH_DATA_DIR` and its state folder as `HORCH_STATE_DIR`. The
  worker finds its brief in the state folder, and the worker's agent
  inherits the worker's environment.
- The binary overrides (`HORCH_*_BIN`) travel in the brief.
  `runtime::bins::BinOverrides` holds the explicit `HORCH_*_BIN` values and
  nothing else. `Brief` has schema 1 agent-CLI override fields, which
  `Brief::overrides` folds into `Brief::bin_overrides`.
- `Brief::teammates_dir` carries where the roster was read from at spawn
  time, because a pane does not inherit `$HORCH_TEAMMATES_DIR`.
  `Roster::load_layered` (`crates/horch-core/src/roster/repository.rs`)
  layers built-ins, then `<home>/.config/horch/teammates`, then
  `roster_override` (`$HORCH_TEAMMATES_DIR`), then `explicit`. A pane gets
  the path as `explicit`, the same way `HORCH_CLAUDE_BIN` travels.
- `runtime::paths::Paths` (`crates/horch-core/src/runtime/paths.rs`) keeps
  the explicit `$HORCH_STATE_DIR` (`state_override`) and passes it on to
  panes. A pane command sets `HORCH_DATA_DIR` to `Paths::data_root`
  (`$HORCH_DATA_DIR`, else `${XDG_DATA_HOME:-$HOME/.local/share}/horch`).
  An explicit `$HORCH_LEDGER_DB` travels in the brief as `ledger_db`.
- `PaneWorker::enter_context` (`crates/horch-core/src/execution/lifecycle.rs`)
  takes the project, the ledger and the binary overrides from the brief and
  applies them before any step that can fail (N-11). A startup failure is
  then recorded in the spawner's ledger, and the pane closes through the
  spawner's herdr.
- `recipes::pane_command_for` (`crates/horch/src/cmd/recipes.rs`) writes
  the roster path and the state folder into the command line the herdr
  server runs. The ledger the record lives in must be the one the pane
  writes.
- `recipes::child_env` passes everything `horch fleet` knew. The state
  folder is the effective one, whether `--state-dir`, the environment or
  `XDG_STATE_HOME` chose it, so the agent's `horch` finds this pane's
  mailbox even when its harness strips ambient variables. When
  `HORCH_PROJECT_DIR` names another directory, the process moves into it:
  the working directory, not an environment variable, so the agent and a
  relative roster path both see it.
- The teammate watch base `compact_at` (CTX-21,
  `crates/horch-core/src/roster/teammate.rs`) is frontmatter only, with no
  environment variable. It is in tokens, 50,000 to 1,000,000, and `horch
  context`, `horch compact` and the `horch note` check use it in place of
  the fleet base. Every built-in teammate on a harness horch watches states
  it (X1). `None` in an overlay warns.
- `PaneShell::line` (`crates/horch-core/src/workspace/paneshell.rs`) calls
  `env` by absolute path on POSIX, because a pane's PATH is not this
  process's PATH. It uses only `env -u NAME` and `NAME=VALUE`, which BSD,
  GNU and busybox `env` all take.
- The e2e tests check this. A fleet with a custom `HORCH_STATE_DIR`
  carries it on the orchestrator's launch line, and the orchestrator
  registers under that folder, not the default state root, and `inbox`
  lists it (`crates/horch-e2e/tests/messaging.rs`). A split pane does not
  inherit `$XDG_DATA_HOME` (G3, LA-5): the worker pane command sets the
  spawner's store as `HORCH_DATA_DIR` (G8; G3 used a brief field), so a
  worker run without the variable still finds a skill installed only in
  that store. The worker's agent inherits `HORCH_DATA_DIR` (G7), and the
  orchestrator pane command sets it too
  (`crates/horch-e2e/tests/skills_exposure.rs`).
