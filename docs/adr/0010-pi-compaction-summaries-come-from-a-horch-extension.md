# pi compaction summaries come from a horch extension

horch loads `crates/horch-core/assets/pi/horch-compact.ts` into pi per
launch with `pi --extension <path>` (X3). It replaces pi's model-written
compaction summary with one built from the session alone: no model call,
no network, no file write, no environment read. pi still picks the cut:
everything after `preparation.firstKeptEntryId` (the newest
`keepRecentTokens`) stays verbatim. The extension summarizes only the
older part, and it keeps the `/compact` argument (horch's keep-list, with
the handoff path) word for word.

## Consequences

- The file has no imports, so it does not depend on pi's package names.
  It uses only erasable TypeScript syntax, because node strips the types
  in the tests.
- The `session_before_compact` hook catches any error and returns
  `undefined`. pi then writes its own summary: a worse summary, not a lost
  one.
- pi loads an explicit `--extension` path also under `--no-extensions`
  (`crates/horch-core/src/harness/pi.rs`). When the file cannot be
  written, the pane starts without it and pi writes its own summary.
- `compact_extension_path` keeps the extension under the state root, named
  by the first 12 hex digits of its SHA-256. A new horch version writes a
  new file, so a live pane's file never changes under it.
- A Codex pane can write the state root (W21). So
  `install_compact_extension` makes the file hold exactly the embedded
  bytes, mode 0600, in dirs of mode 0700. An existing file is compared byte
  for byte, and anything else (other bytes, a link, another mode) is
  replaced with a temp name, then a rename.
