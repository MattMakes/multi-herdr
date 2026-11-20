# Pane Host Specification

`horch` drives one of two pane hosts per fleet: `herdr` (the `herdr` server behind the herdr TUI) or `hc` (`herdr-core` plus `herdr-app` behind the Herdr desktop app).

## Requirements

| ID | Requirement |
|---|---|
| HCB-01 | The backend is resolved once in `RuntimeContext` from `--backend`, else `HORCH_BACKEND`, else whichever single pane identity (`HERDR_PANE_ID` or `HC_TERMINAL_ID`) is set, else `herdr`; setting both pane identities without an explicit backend is refused as ambiguous, and `HerdrEnv.pane` reads from the resolved backend's variable. |
| HCB-02 | `Brief::transport_env` carries `HORCH_BACKEND` and any `HORCH_HC_BIN` / `HORCH_HERDR_APP_BIN` overrides to worker panes, while detached children scrub `HC_TERMINAL_ID`, `HC_SCOPE`, and `HORCH_BACKEND` alongside the starting pane's identity. |
| HCB-03 | `Execution` records and task ledger `workers` rows store `backend` (`herdr` or `hc`, defaulting to `herdr` and omitted from JSON when `herdr` so existing records keep their bytes), schema migration adds `workers.backend`, and `ExecutionStore::backend_of_workspace` returns the recorded owner of a workspace. |
| HCB-04 | Every host call outside `horch_core::workspace` goes through `PaneHost` (`of`, `for_backend`, `for_workspace`, `herdr_bin`), with no direct `Herdr::with_bin` or `workspace::herdr::Herdr` references outside `crates/horch-core/src/workspace/`, and `PaneHost::for_workspace` selects the workspace's recorded backend or falls back to `ctx.herdr.backend`. |
| HCB-05 | `HcApp` maps an `hc` terminal to `Pane` (`address` -> `pane_id`, `herdr.space` / `herdr.tab` labels -> `workspace_id` / `tab_id`, `status.app` / `status.state` -> `agent` / `agent_status`, `status_ms` -> `state_change_seq`), treats `state: "finished"` and exit `3` as gone, bounds `pane_list` (`hc ls` + per-terminal `hc get`) under a single deadline, maps `pane_read` and `wait_output`, and checks `core_socket` before running `hc server status` so reachability probes never start a core. |
