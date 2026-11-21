# Roster check refuses teammate settings an agent cannot honour

A teammate file must mean exactly what it launches. Where a setting has no
equivalent on a teammate's agent, `--check`
(`crates/horch-core/src/roster/validation.rs`) and the flag mapping in
`crates/horch-core/src/roster/permission.rs` refuse it instead of
approximating it, so no worker silently runs with more permission or less
restraint than its file states.

## Consequences

- `Teammate` (`crates/horch-core/src/roster/teammate.rs`) uses serde
  `deny_unknown_fields` deliberately. A misspelled key in a hand-written
  teammate file would otherwise be silently ignored, producing a worker
  launched with permissions or tools its author believed were set.
- `PermissionMode::opencode_args`: OpenCode has 1 lever, `--auto`
  ("auto-approve permissions that are not explicitly denied"), so the modes
  collapse into passing it or not. `Auto`, `BypassPermissions` and
  `AcceptEdits` pass it. `Plan`, `Manual` and `DontAsk` have no analogue and
  return `None`, which is an error: OpenCode has no read-only mode, and a
  worker left to prompt in a pane nobody is watching stalls forever.
- `AcceptEdits` maps to `--auto` because OpenCode still asks about
  anything explicitly denied, the closest it has to "accept edits, ask for
  the rest".
- `PermissionMode::antigravity_args`: `agy` has `--mode plan|accept-edits`
  and 1 bypass, `--dangerously-skip-permissions`. `Auto` adds `--sandbox`,
  which confines terminal commands, to the bypass: approve everything,
  inside the sandbox. `Manual` and `DontAsk` have no analogue.
- `PermissionMode::codex_args` gives the plain codex sandbox and approval
  flags. The write modes never ask: a command the sandbox refuses fails
  back to the agent, so nothing outside the pane's folder can be granted.
  The codex adapter swaps the write modes for its fleet permissions profile
  (`harness::codex::permission_args`). `Manual` and `DontAsk` have no codex
  analogue and return `None`, so the caller raises an error rather than
  silently downgrading a teammate to something more permissive than its
  file asked for.
- `permission_mode_rules` refuses any `permission_mode` on pi and Prime
  teammates. They run their tools without asking, so there is no gate for a
  mode to set. Saying nothing is correct, and saying `acceptEdits` implies a
  restraint that is absent.
- `sandbox_rules` accepts the `sandbox` field only on claude teammates.
  Only Claude has an OS sandbox horch can switch on per launch. On any
  other agent the field would promise a boundary that never exists.
- `harness_field_rules` rejects `tools:` on a Prime teammate. pi turns
  `tools` into `--tools` or, for an empty list, `--no-tools`. The Rust build
  of Prime refuses `--tools`, `--no-tools` and `--exclude-tools`, so any
  `tools:` value would exit the pane at launch.
- `args_rules` refuses an `args` list that ends on a variadic flag
  (`--mcp-config`, `--tools`, `--allowedTools`, `--allowed-tools`,
  `--disallowedTools`, `--disallowed-tools`). `args` is emitted immediately
  before the prompt, and a variadic flag would take the prompt as 1 more
  value.
- `antigravity_rules` refuses the `ANTIGRAVITY_FORBIDDEN_ENV` keys
  (`GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_GEMINI_BASE_URL`) on an `agy`
  teammate. An `agy` child signs in with the operator's Google login, these
  keys bypass it, and the launch removes them from an `agy` child as well.
