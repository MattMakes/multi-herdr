# The claude command line is built in a fixed flag order

`claude_command` in `crates/horch-core/src/harness/claude.rs` builds every
Claude Code launch in a fixed order. `--mcp-config`, `--tools`,
`--allowedTools` and `--disallowedTools` are variadic in the claude CLI
(`<x...>`): they keep consuming arguments until the next flag. The prompt
is a bare positional at the end, so a variadic flag that came last would
swallow it as 1 more config path or tool name. Every variadic flag goes
first (`push_mcp_args`, `push_tool_args`), and `--model`, which is always
present and never variadic, fences them off from the prompt.

## Consequences

- The teammate's escape-hatch `args` sit right before the prompt. A
  variadic flag at the end of `args` would eat the prompt, and
  `Roster::check` warns about that case.
- `Claude::expose_skills` appends the skills bundle to `plugin_dirs`. The
  builder emits `plugin_dirs` before the scalar flags, which fences the
  variadic `--plugin-dir` away from the briefing prompt.
- Teammate `tools: Some([])` means "no tools" and maps to `--tools ""`.
  Omitting the flag would give claude its full default tool set.
- `push_mcp_args` always pairs `--mcp-config` with `--strict-mcp-config`.
  Strict mode makes `--mcp-config` the whole truth rather than an addition,
  so an empty `mcpServers` object means "no MCP servers at all". Without
  it the operator's own MCP servers would still load.
- Haiku 4.5 has no effort setting, and claude rejects `--effort` for it.
  `Claude::model_takes_effort` returns false for any model name with a
  `haiku` segment, and `push_model_args` drops the effort. A
  `--model haiku` override can land on a teammate whose file sets an
  effort, and dropping it is better than failing the launch.
- `install_system_prompt` writes the teammate's `system_prompt` text under
  the state root, named by the prompt and the first 12 hex digits of its
  SHA-256, and passes it with `--append-system-prompt-file`. The name is
  content-addressed, so the file of a live or resumed pane never changes
  under it when the prompt text is edited.
