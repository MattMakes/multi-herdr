# Ledger paths are stored and matched in canonical form

The same folder can be spelled more than 1 way: on macOS `/tmp` is
`/private/tmp` and `/var` is `/private/var`, while git, Seatbelt and the
OS report the real, resolved paths. So horch canonicalises a path before
it compares it with git's worktree paths. `runs::root_key` is the 1
spelling a project root is stored and matched by: canonical when the
folder exists, else as given. Otherwise 2 spellings would make 2 projects.
The tests and the e2e fakes canonicalize paths the same way.

## Consequences

- `import::known_worktrees` returns the canonical paths of every worktree
  the ledger names, because git can report a worktree through a different
  spelling than its claim recorded. Comparing raw strings would report
  live worktrees as orphans. The import test spells each old record's path
  through a symlink, and it must still match git's real paths
  (`crates/horch-e2e/tests/taskledger.rs`).
- A pane started with a symlinked `--cwd` runs from the canonicalised cwd,
  which can differ from the given path (`/tmp` and `/private/tmp` on
  macOS). The hc pane-launch test runs the launch line, inside the root
  terminal with `HC_TERMINAL_ID` set and no `HERDR_PANE_ID`, from the
  canonicalised cwd, and checks the agent gets `HORCH_BACKEND=hc` and no
  `HERDR_PANE_ID`.
- The tests in `crates/horch-core/src/harness/claude.rs` that compare a
  path with git's output canonicalize the temp dir first, because the
  macOS temp folder is a symlink (`/var` -> `/private/var`).
- `fake-codex` (`crates/horch-e2e/src/bin/fake-codex.rs`) canonicalizes
  the writable roots before it builds the Seatbelt profile, because
  Seatbelt matches resolved paths.
- The session id `fake-opencode` reports for a worker in `dir` is `ses_`
  and `hash16` of the canonical directory path
  (`crates/horch-e2e/src/lib.rs`), because the fake sees the path the OS
  reports. Fake codex session ids hash the canonical cwd the same way.
