# Identifiers are typed values with fixed formats

`crates/horch-core/src/ids.rs` defines each identifier as its own type
with a fixed format. New ids are UUIDv7, minted by hand in `mint_v7` from
uuid v4 randomness (OD7), so the uuid crate's enabled feature set stays as
it is. Bytes 0..6 hold the big-endian Unix millisecond timestamp, so ids
sort by time. The version nibble is 7 and the RFC 4122 variant bits are
kept from the v4 source. The remaining 74 bits are random.

## Consequences

- `ExperimentId::short` is the `<exp8>` part of candidate branch names
  (`mh/exp/<exp8>/...`): the last 8 letters and digits of the id. A UUIDv7
  starts with its millisecond timestamp, so the head is shared by
  experiments started close together. The tail is random.
- `RoleName` rejects `/`, `\` and `..`. A role names a mailbox file and a
  pane, so it must stay 1 path segment and can never escape its
  directory.
