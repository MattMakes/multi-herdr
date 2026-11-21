# A closing pane re-tiles from a detached child in a new session

`herdr pane close` takes down the calling pane's whole process tree, so a
worker shutting itself down cannot re-tile in-process: by the time the hole
it leaves exists, it has been killed. `arrange::settle_after_close`
(`crates/horch-core/src/workspace/arrange.rs`) hands the job to a detached
child that outlives the pane. The child fills the hole and lets herdr close
an emptied overflow tab.

## Consequences

- The child is started with `spawn_detached`
  (`crates/horch-core/src/runtime/process.rs`), which puts it in a new
  session, so it survives the close of the pane that started it.
