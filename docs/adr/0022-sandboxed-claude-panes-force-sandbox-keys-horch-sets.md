# Sandboxed Claude panes force the sandbox keys horch sets

A teammate's `sandbox:` block turns on Claude Code's sandbox for its pane.
`SANDBOX_FORCED` in `crates/horch-core/src/harness/claude.rs` lists 4
`sandbox` keys, as (path, value), that horch sets on every sandboxed
launch whatever the teammate file says, and `check` rejects a file that
sets one to anything else. Each closes a way out of the sandbox:

- `enabled: true`: the field means "sandbox this pane".
- `failIfUnavailable: true`: Claude Code exits at startup rather than run
  commands unsandboxed.
- `allowUnsandboxedCommands: false`: no retry outside the sandbox. Given
  through `--settings`, it also makes the sandbox admin-required, so a
  repository's `.claude/settings.json` cannot widen it.
- `network.strictAllowlist: true`: a host outside `allowedDomains` is
  denied in every permission mode, auto mode's per-command domains
  included.

## Consequences

- `sandbox_problems` reports, 1 line each, what in a teammate's
  `sandbox:` block would open a way out: a value against a
  `SANDBOX_FORCED` key, `excludedCommands` (Claude Code runs those
  commands outside the sandbox) or `filesystem.disabled`.
- `sandbox_host_problem` refuses a sandboxed launch unless the host can run
  Claude Code's sandbox. macOS needs Seatbelt's `/usr/bin/sandbox-exec`.
  Linux needs `bwrap` and `socat` on `PATH`
  (code.claude.com/docs/en/sandboxing, "Set up Linux and WSL2"). The
  sandbox runs on no other OS.
- `inline_settings_arg` refuses a sandboxed teammate whose `settings` is
  a file path. A skills launch gives inline JSON with the sandbox merged
  in, and a file the teammate named would replace horch's overlay and drop
  the sandbox.
