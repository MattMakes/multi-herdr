# Codex fleet launches override config with -c pairs

The codex launch command in `crates/horch-core/src/harness/codex.rs` sets
the settings a fleet pane needs as `-c key=value` pairs on the command
line. Fleet panes must reach their briefing without startup dialogs, so
every launch sets `check_for_update_on_startup=false` and
`tui.resume_cwd="current"`. Trust for enabled hooks is set separately
(`push_hook_trust`) from the sandbox and command approval policy.

## Consequences

- When multi-agent is off, `write_v1_catalog` writes a copy of codex's
  live model catalog (from `codex debug models`) that pins every model to
  the v1 multi-agent protocol, and the launch points
  `model_catalog_json` at it. Codex otherwise exposes v2 collaboration
  tools even when `features.multi_agent=false`.
