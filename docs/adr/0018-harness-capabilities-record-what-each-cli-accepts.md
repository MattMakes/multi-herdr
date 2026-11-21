# Harness capabilities record what each CLI accepts

`Capabilities` in `crates/horch-core/src/harness/capabilities.rs` holds,
per harness, which flags and features its CLI really supports, and horch
uses a feature only where the harness's entry allows it. A flag a CLI
does not know makes the launch fail, and a feature a CLI cannot scope to
1 pane would leak shared state into it.

## Consequences

- `extensions_flag` (`--no-extensions`) and `tool_selection` (`--tools`,
  `--no-tools`, `--exclude-tools`) are true only for pi. The Rust build of
  Prime Agent (0.10.0) has no extension loader and refuses all 4 flags
  with exit 1, and no other harness has them.
- The Antigravity CLI (`agy`, `ANTIGRAVITY`) exposes no bundled skills
  (`SkillExposure::None`). It has no flag or environment variable that
  points it at a skills directory. It reads only the operator's global
  skills and the workspace `.agents/`, both shared state.
- `agy` mints its own conversation ids, so horch discovers its session
  after launch.
