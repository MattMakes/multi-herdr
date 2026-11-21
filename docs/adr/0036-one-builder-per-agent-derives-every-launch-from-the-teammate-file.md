# One builder per agent derives every launch from the teammate file

`crates/horch-core/src/harness/launch.rs` turns a `Teammate` into an
agent-CLI command line, with 1 builder per agent shared by every caller.
Everything is derived from the teammate's frontmatter, and no call site
hard-codes launch flags. `horch worker` and `horch pane-launch` used to
build their own argv, which is how the orchestrator ended up with an effort
level and a model that appeared nowhere in any file.

## Consequences

- The prompt is the last positional, and nothing may swallow it. Claude's
  `--mcp-config`, `--tools`, `--allowedTools` and `--disallowedTools` are
  variadic, so the builder fences them off with `--model`. A variadic flag
  anywhere after `--model` could reach the prompt, and a live probe lost
  the prompt this way.
- The codex orchestrator is the 1 pane that launches codex with a model, an
  effort and a sandbox all at once. The prompt must still land last: a
  briefing swallowed by an earlier flag is an orchestrator that comes up
  with no instructions and no error.
- A codex write pane selects the fleet write profile and never also passes
  `-s`/`--sandbox`: codex would switch back to its legacy sandbox and drop
  the profile.
- Claude Haiku has no effort setting and the claude CLI refuses `--effort`
  for it. A model override onto Haiku drops `--effort` instead of launching
  a claude that refuses the flag.
- `launch_command` drops the state root, and so the codex v1 model catalog,
  when catalog regeneration failed. It does not reuse an older catalog:
  Codex can auto-update between launches, so that file may no longer match
  it.
- `launch_command` applies the adapter's `Prepared.env` last, after every
  other environment layer, so a value the harness prepared (codex's private
  home, Prime's agent dir) wins. The window read-back comes after it,
  because Prime's window is in the agent dir its `prepare` names in the
  environment.
- `HC_STATUS_HARNESSES` lists the harnesses (Claude, Codex, Antigravity)
  measured in Task 1 (Q3) to report OSC 7501 status transitions (`idle` ->
  `working` -> `idle`/`done`) under the `hc` pane host. `refusal_on_hc`
  refuses to spawn any other real harness on `hc`, because it could not
  report status there.
