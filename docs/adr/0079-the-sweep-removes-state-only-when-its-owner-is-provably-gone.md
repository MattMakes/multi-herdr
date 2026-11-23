# The sweep removes state only when its owner is provably gone

horch writes state per workspace and per launch, and the process that owns
it usually dies by signal, so no `Drop` removes it (#140). The sweep in
`crates/horch-core/src/sweep.rs` is 1 pass that finds the state whose
owner is provably gone and removes it. Anything it cannot prove stale
stays.

| kind        | where                                  | stale when                                            |
|-------------|----------------------------------------|-------------------------------------------------------|
| record      | session records (task ledger)          | `working`, and herdr answers without its workspace or pane |
| spool       | `<state>/messages/<ws>/`               | older than 7 days, and no pending mail names it       |
| socket      | `<state>/channels/*.sock`              | no relay answers a ping                               |
| registry    | `herdr-orchestration-<ws>` (state, temp) | herdr answers without `<ws>`, and `<ws>` has no pending mail |
| mail        | `<state>/mail/<ws>/`                   | as for the registry                                   |
| bundle      | `<state>/skill-bundles/<record id>/`   | its record is terminal                                |
| codex home  | `<state>/codex-home/<role>-<uuid>/`    | every record of the role is terminal, and codex left no stranded file |
| temp file   | `write_atomic` temp files              | its pid is dead, and it is older than 1 hour          |

## Consequences

- A herdr that does not answer leaves all state alone. `Sweeper::run`
  removes nothing unless at least 1 pane host answered its workspace
  list, because without an answer no owner can be proven gone.
- A skill bundle that no record names stays: its owner is unknown.
- `SETTLE` is 1 hour. A workspace dir, a codex home or a temp file younger
  than that stays, because its owner may still be starting while the
  sweep reads.
- The sweep runs at `horch fleet` and `horch spawn` at most once an hour
  (`run_due`). `horch doctor` counts what it would remove (`scan`), and
  `horch doctor --clean` removes it (`Plan::apply`).
- `run_due` writes the `.sweep-stamp` file before it sweeps, so 2 spawns
  that start at once do not both sweep.
- `scope_of` sets `temp_root` only for the default state root. The
  temp-dir registries belong to it, and a moved state root (a test, a
  second install) must not judge or delete them.
