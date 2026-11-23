# A herdr hidden from stat runs by its absolute path

`herdr_bin` in `crates/horch-core/src/runtime/bins.rs` picks the herdr CLI
to drive. A sandboxed Claude pane may run `herdr` but may not stat it.
There the libc PATH search of a bare `herdr` runs it once for each matching
PATH entry and then fails with "not found", so horch gets no answer from a
herdr call that ran. When no `herdr` on PATH can be read, horch runs the
first absolute PATH entry whose `herdr` is hidden by a permission error, by
its absolute path.

## Consequences

- `$HORCH_HERDR_BIN` overrides the choice. The end-to-end tests use it to
  put a recording fake in place of herdr.
