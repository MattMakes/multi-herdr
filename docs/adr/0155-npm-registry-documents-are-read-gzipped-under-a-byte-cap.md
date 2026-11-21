# npm registry documents are read gzipped under a byte cap

`crates/horch/src/cmd/update.rs` reads each npm package's registry document
for the publish time of each release (the release-cooldown lookup).
`update::npm_packument` asks for gzip, because the largest document
(`opencode-ai`'s) shrinks from about 25 MB to 4 MB, which keeps the lookup
within its 60 s `REGISTRY_TIMEOUT`.

## Consequences

- horch reads at most `MAX_PACKUMENT_BYTES` (256 MiB) of 1 registry
  document, before and after gzip.
- `update::read_packument` decides gzip by the magic bytes `0x1f 0x8b`,
  not by the Content-Encoding header, since JSON never starts with them.
  The decoded stream also stops at `MAX_PACKUMENT_BYTES`, which bounds a
  gzip bomb.
