# Detached children start a new session

`spawn_detached` in `crates/horch-core/src/runtime/process.rs` starts a
child that outlives its starting pane in a new session (`setsid`), not
merely a new process group. Measured against herdr 0.7.4, closing a pane
tears down its whole session. A child that only left the process group is
killed along with it, while one that called `setsid` survives.

## Consequences

- `spawn_detached` scrubs the child's environment (`scrub_child_env`)
  right before the spawn. No detached child (the judge job, the compaction
  job, the grid tidy) can carry a forbidden key, whatever its caller set.
- `kill_group` kills the group of a child started with `process_group(0)`
  by the child's pid. The child must not be reaped yet, so its pid still
  names its group.
- Off unix there is no process group, so `kill_group` kills only the
  child.
