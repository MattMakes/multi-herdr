# Pane command lines drop `FORBIDDEN_ENV` before they run

`crates/horch-core/src/workspace/paneshell.rs` quotes command lines handed to
`herdr pane run`. `pane run` submits literal text that the pane's own shell
parses, and that shell is whatever herdr starts panes with per platform
(POSIX sh or PowerShell), not this process's shell. Getting the quoting
wrong is silent: the pane shows a shell error nobody reads. The dialect is a
parameter rather than a `cfg`, so both branches are testable on either
platform. Every line first removes `FORBIDDEN_ENV`. The herdr server keeps
the environment of the shell that started it, so a pane shell can hold
`ANTHROPIC_API_KEY`, and no process in a pane horch starts may hold it
(U-68).

## Consequences

- A pane does not inherit this process's environment.
  `PaneShell::command_line_with_env` sets each `(name, value)` explicitly for
  `exe` and every process it starts.
- On POSIX the line `exec`s, so `exe` replaces the pane's shell and no shell
  holding a `FORBIDDEN_ENV` variable stays in the pane. The pane ends when
  `exe` ends. A pane that must run more lines after `exe` uses
  `PaneShell::command_line_keep_shell`, which `unset`s the variable in the
  shell first.
- On PowerShell, `Remove-Item Env:X` removes the variable from the PowerShell
  process itself, so the pane shell loses it too and no `exec` is needed.
  The command is invoked with the `&` call operator, because PowerShell
  otherwise treats a quoted first token as a string expression and just
  echoes it.
- The pane command removes the key before horch starts. The e2e test
  (G3, `crates/horch-e2e/tests/lifecycle.rs`) runs the recorded line again
  in a shell that holds the key, with a probe in place of horch, and the
  probe does not see the key.
- The U-68 tests append a second line, which runs only if the shell stayed.
  `sh -c` with 1 command can exec it without being told, so a 1-line test
  proves nothing.
