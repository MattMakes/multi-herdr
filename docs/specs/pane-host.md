# Pane Host Specification

`horch` drives one of two pane hosts per fleet: `herdr` (the `herdr` server behind the herdr TUI) or `hc` (`herdr-core` plus `herdr-app` behind the Herdr desktop app).

## Requirements

| ID | Requirement |
|---|---|
| HCB-01 | The backend is resolved once in `RuntimeContext` from `--backend`, else `HORCH_BACKEND`, else whichever single pane identity (`HERDR_PANE_ID` or `HC_TERMINAL_ID`) is set, else `herdr`; setting both pane identities without an explicit backend is refused as ambiguous, and `HerdrEnv.pane` reads from the resolved backend's variable. |
| HCB-02 | `Brief::transport_env` carries `HORCH_BACKEND` and any `HORCH_HC_BIN` / `HORCH_HERDR_APP_BIN` overrides to worker panes, while detached children scrub `HC_TERMINAL_ID`, `HC_SCOPE`, and `HORCH_BACKEND` alongside the starting pane's identity. |
