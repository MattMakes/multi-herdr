# Codex write panes run in the fleet_write permissions profile

A codex write pane launches in `FLEET_WRITE_PROFILE` (`fleet_write`), a
permissions profile that `fleet_write_profile_args` in
`crates/horch-core/src/harness/codex.rs` defines with `-c` pairs. It extends
`:workspace`. The pane's folder (codex's project roots) is writable
throughout, `.git`, `.codex`, `.agents` and `.aws` too, which `:workspace`
keeps read-only, so a codex worker can commit and make worktrees.
Everything else stays read-only, `/tmp` and the system temp dir as well.
`state_root` is the 1 extra writable root: `horch note` and `horch done`
write the ledger in it, and the pane's `TMPDIR` lives under it. The network
is on. The sandbox itself keeps processes apart: a sandboxed command can
signal only processes in its own sandbox (Seatbelt `signal (target
same-sandbox)`, a PID namespace on Linux).

## Consequences

- `pane_tmpdir` gives each pane a private temp dir,
  `<state root>/tmp/<role>`, because `/tmp` and the system temp dir are
  read-only in the profile.
- A plan pane defines the profile too. Codex lists a permissions profile in
  its `/permissions` menu only if it was defined at launch, and
  `horch mode <role> write` selects it there (X6 repro 5). The profile is
  defined in `prepare`, which knows the state root.
- `permission_args` gives a plan pane in the profile form
  `default_permissions=":read-only"`, not `-s read-only`. Only then does
  `/permissions` list Read Only, so `horch mode <role> plan` can switch
  back (X6 repro 3). It returns `None` for no mode, or a mode with no codex
  equivalent.
- A write pane selects the profile and never asks for approval.
- `write_profile_form` and `plan_profile_form` use the profile form only
  when the teammate's own args set no sandbox and no profile, because Codex
  refuses `sandbox_mode` and `default_permissions` together. A teammate
  whose own args choose a sandbox gets the plain flags, and its own args,
  later in the argv, win.
- Every mode gets `--search`, codex's live web search tool.
- `sandbox_args` runs a command in the sandbox of a codex write pane (#181)
  through `codex sandbox`, with the profile, `dir` as its project root and
  `state_root` as its 1 extra writable root. The command follows the last
  arg, `--`. `codex sandbox` keeps the command in its own process group, so
  killing the group of `codex` kills the command too.
- `status_permissions` reads the newest `Permissions:` line of a `/status`
  block and joins its wrapped parts. Codex wraps that value at narrow pane
  widths, even mid-path. A wrapped part is indented under the value, and
  the next key is not.
- `status_shows` checks for `Read Only ...` for `Plan` and
  `Profile fleet_write` for `Write`, ignoring all whitespace, because a
  wrap can split any word. `/status` does not list a profile's roots, so
  only the profile name can be checked (X6 repro 5).
