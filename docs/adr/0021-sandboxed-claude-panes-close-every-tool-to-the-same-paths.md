# Sandboxed Claude panes close every tool to the same paths

Claude Code's sandbox wraps Bash only. A sandboxed pane must keep the Read,
Grep, Glob, Edit and Write tools inside the same bounds as its shell, so
each path horch opens or closes is stated twice: once in
`sandbox.filesystem` for Bash and once in `permissions` for the file
tools. `overlay_sandbox` in `crates/horch-core/src/harness/claude.rs` puts
the teammate's `sandbox:` block into the `--settings` object and sets
`permissions.blockReadsOutsideWorkingDirectories`. Without it the Read,
Grep and Glob tools read what the sandbox denies to a shell command.

## Consequences

- With the sandbox on, `blockReadsOutsideWorkingDirectories` also closes
  the home directory to sandboxed commands. The block's `allowRead`
  re-opens what the teammate needs.
- `overlay_sandbox_dirs` makes each `allowRead` and `allowWrite` path of
  the block, with `~` expanded, a working directory
  (`permissions.additionalDirectories`). Otherwise Claude Code asks before
  any Bash command that names such a path, because of
  `blockReadsOutsideWorkingDirectories` (ticket 31; live probe, Claude
  Code 2.1.318). horch's own paths stay out: only `horch` commands use
  them.
- A working directory is open to Edit and Write too, so an
  `allowRead`-only path gets an `Edit(...)` deny rule. In Claude Code an
  `Edit(...)` deny closes a path to both Edit and Write, while a
  `Write(...)` deny has no effect (ticket 28 finding).
- A permission rule path that starts with `//` is absolute (a single `/`
  is relative to the settings file). The deny rules are formatted as
  `Edit(/{dir}/**)` or `{tool}(/{dir}/**)`, where `dir` already starts
  with `/`.
- `deny_writes` closes a directory to writes for every tool: Bash by
  `sandbox.filesystem.denyWrite`, which wins over `allowWrite`, and Edit
  and Write by deny rules. This closes a directory even inside a writable
  state root.
- `overlay_bundle_paths` opens the launch's own skills bundle to read and
  closes it to write, for every tool (ticket 27;
  `docs/security/sandboxed-pane-horch-access.md`, R6). The Read tool reads
  it as a working directory, because a `Read(...)` allow rule does not pass
  `blockReadsOutsideWorkingDirectories` (live probe, Claude Code 2.1.318).
  Bash gets it in `sandbox.filesystem.allowRead` and `denyWrite`, and
  `denyWrite` wins over the state root in a `full` pane's `allowWrite`.
- `overlay_roster_dirs` opens each roster folder `horch spawn` reads,
  `~/.config/horch/teammates` and the briefed `$HORCH_TEAMMATES_DIR`, each
  when it exists, to read and closes it to write (ticket 35). A pane that
  could edit the roster could re-brief the roles it spawns (R6). Bash gets
  `allowRead` and `denyWrite`, the Read tool a working directory
  (ticket 31), and an `Edit(...)` deny closes it to Edit and Write.
- Claude Code denies `.git` writes inside the working directory by
  default. `overlay_horch_paths` adds the git common dir (for commits) and
  the project's `.worktrees` folder (for `git worktree add`) to
  `sandbox.filesystem.allowWrite` of a sandboxed ledger worker.
