# A Codex worker's check runs in the sandbox of its pane

`horch ledger finish` runs outside a Codex worker's sandbox, and the check
runs code the worker wrote. So for a Codex worker, `finish::CodexSandbox`
and `land::check_process` run the check `command` in a shell inside the
`codex sandbox` of its pane, writable only in the worktree and the state
dir, not with the operator's full rights (#181). A check that writes
outside fails, and the file is never made. The same check of a Claude
worker runs unsandboxed and writes the file, because its host already
allows it.

## Consequences

- A sandboxed check still has the task's timeout (#181 with #160).
  `finish` fails the check with the timeout, and neither the check's
  processes nor the `codex sandbox` parent outlive it.
- `/tmp` is read-only inside the Codex sandbox, so the inner shell gets the
  pane's TMPDIR. Only the shell gets it: `codex sandbox` reads its own
  `TMPDIR` for the profile's `":tmpdir"="read"` entry, so a pane TMPDIR on
  `codex` makes that dir read-only for the check. A `finish` run from a
  Codex pane inherits the pane TMPDIR, so `codex` runs without one.
- Reviewers write into `codesweep::review_root`, `<project>/.horch/reviews`,
  which ignores itself in git. A Codex sandbox may write its working tree,
  not the ledger's evidence folder.
