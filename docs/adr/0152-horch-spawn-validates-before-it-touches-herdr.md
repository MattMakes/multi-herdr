# horch spawn validates before it touches herdr

`horch spawn` (`crates/horch/src/cmd/spawn.rs`) is the spawn gate, and it
decides before anything else happens. In `spawn::launch` the advisory
`fleet_warnings` probes run only after validation, so a refused or headless
request never contacts herdr or creates diagnostic state.

## Consequences

- Reviewers are spawned the same way. `taskledger_spawn::CliSpawn` runs
  `horch spawn <persona> --run <slug>` as a child process instead of
  calling the spawn code in-process, so the spawn gate decides and its
  REFUSED, NOTE and SUBSTITUTED lines stay off the parent command's stdout.
  `HORCH_CLAIM_TARGET` (`CLAIM_TARGET_ENV`) reserves the review task for
  the new worker.
