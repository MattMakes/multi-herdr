# The herdr wrapper targets herdr 0.6.1, and only tiling needs 0.8.2

`crates/horch-core/src/workspace/herdr.rs` is a typed wrapper around the
`herdr` CLI that replaces the shell's `herdr ... | jq` pipelines. It is
verified against herdr 0.6.1 and re-checked against 0.8.0. The splitting and
messaging surface is limited to what 0.6.1 advertises: no `pane current`,
and no `--env` or `--ratio` on `pane split`. The tab and `pane move` calls
that `horch tile` needs arrived later and are verified against 0.8.2. They
are additions, so a caller that never tiles still only needs 0.6.1.

## Consequences

- `Herdr::tab_list` always passes `--workspace <ws>`. Without it
  `herdr tab list` returns the tabs of every workspace herdr has open. Tabs
  are returned in tab-strip order (`number`).
- `Herdr::wait_output` runs `herdr pane wait-output <pane> --match <needle>
  --source recent-unwrapped --timeout <ms>` and returns false on timeout.
  herdr 0.8.2 has no top-level `wait`: the old `herdr wait output` exits 2
  with `unknown command: wait`, which read as a timeout and failed every
  `horch smoke messaging`. Only herdr's `{"error":{"code":"timeout"}}` counts
  as no match (`wait_outcome`). Any other failure is an error. The source is
  the one the smoke failure dump prints.
- `HERDR_PANE_ID` holds herdr's internal pane id, and the commands need the
  public one. `arrange::balance_target` round-trips it through
  `herdr pane get`. It resolves the layout like `horch layout`: an explicit
  pane, else any pane in the named workspace, else this pane.
