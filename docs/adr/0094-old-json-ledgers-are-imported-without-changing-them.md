# Old JSON ledgers are imported without changing them

`taskledger::import` imports the old JSON ledgers (section 11, #43) and lists
the work that never landed (`horch ledger unfinished`). The source files are
never changed, and the import is idempotent: a second import skips what it
already has.

## Consequences

- Stage 1 copies every old record unchanged into `import_raw`.
- Stage 2 maps each unmapped raw row to typed rows with `origin = 'import'`,
  where required fields are not enforced. A row that cannot be mapped keeps
  its raw copy with `map_error`.
- Stage 1 also copies each session ledger record into `session_records`
  (TRF-45), where the session readers and writers find it. A record that
  table has already keeps its row: a later write there is newer than the
  JSON file.
