# Digests hash canonical JSON with sorted keys

Digests that name tasks, configs, judge policies and environments must be
identical on every machine and every run. `digest::write_canonical` in
`crates/horch-core/src/digest.rs` sorts object keys itself rather than
relying on the map order of `serde_json`.

## Consequences

- `serde_json`'s `preserve_order` feature can be switched on by any crate
  in the build, so the map order is not under horch's control.
