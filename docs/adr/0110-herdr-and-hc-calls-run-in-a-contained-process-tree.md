# herdr and hc calls run in a contained process tree

`crates/horch-core/src/workspace/exec.rs` runs each `herdr` or `hc` child so
that no descendant can outlive the call on any path. On Windows the child
runs inside a job object that kills every process in the tree when the last
handle closes. On Unix the child leads its own process group, and on failure
`kill_and_reap` signals the whole group. A descendant that holds a pipe open
would otherwise keep the call, or its reader threads, alive.

## Consequences

- `output_within` starts the Windows child suspended (`CREATE_SUSPENDED`), so
  it cannot start a descendant before the job holds it.
- `resume_main_thread` finds the suspended child's only thread by its owner
  process, because Rust std keeps the main thread handle of a spawned child
  to itself.
- `has_exited` checks for exit without reaping (`waitid` with `WNOWAIT`
  semantics). The child stays a zombie, so its pid and process group cannot
  be reused before the caller kills and reaps it.
- `kill_and_reap` signals the group while the leader is still unreaped, so
  the pid cannot belong to another process and the signal reaches only this
  call's descendants. A failure to kill or reap is a warning, not an error:
  the call has already failed and the caller goes on.
- `READER_GRACE` (200 ms) is the most a finished call waits for its reader
  threads after the child is killed. A reader still blocked then is left to
  end with its pipe, because a descendant that escaped containment can hold
  the pipe open.
