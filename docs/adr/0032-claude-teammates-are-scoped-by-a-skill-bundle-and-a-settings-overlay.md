# Claude teammates get a per-launch skill bundle and keep operator settings

A Claude teammate keeps the operator's settings and gets its scope from 2
things horch adds per launch (`crates/horch-core/src/harness/launch.rs`): a
skill bundle with native skill discovery plus a short routing instruction,
and a `--settings` overlay. The orchestrator keeps the operator's settings,
which are already tuned for token economy, and sheds only plugins and MCP
servers. It does not shed skills: `--disable-slash-commands` also removes
the built-in `/context` and `/config`, and there is no way to keep just
those 2.

## Consequences

- A teammate's `mcp_servers` holds inline MCP server definitions. `None`
  leaves the operator's MCP configuration alone. `Some` renders
  `--mcp-config <json> --strict-mcp-config`, so `Some({})` means exactly 0
  MCP servers. A launch with declared MCP servers always passes
  `--strict-mcp-config`. It is what makes `--mcp-config` subtractive.
  Without it the operator's servers load too and the isolation is a no-op.
- Teammate files are committed, so they hold no secrets: a server needing
  an API key belongs in `mcp_config_files`, pointing at a file the
  operator owns.
- The launcher holds the `skills::Bundle` (`PreparedLaunch.skills`) until
  the agent child exits. Agents read skill files lazily, and dropping the
  bundle removes its directory.
- `bundle_name` names the bundle directory after the ledger record id only
  when the id is 1 plain path component (`ExecutionId`) that no directory
  in the bundles dir has yet. Otherwise it uses a fresh execution id. A
  legacy record id can hold `/` or `..`, which must never name a directory.
- `write_briefing_skill` creates the orchestrator skill directory with
  `create_dir`, not `create_dir_all`. A catalog skill of the same name then
  makes the write fail rather than being overwritten.
