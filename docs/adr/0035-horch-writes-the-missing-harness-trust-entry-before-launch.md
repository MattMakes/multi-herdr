# horch writes the missing harness trust entry before launch

Claude, Codex and Antigravity show a trust dialog on their first launch in
a folder, and a pane stopped at it does no work (PRE-14, ARC-29). So
`ensure_trusted` in `crates/horch-core/src/harness/trust.rs` reads the
harness's trust store and, before an agent starts, writes the 1 missing
entry for the agent's workdir. It writes only when the store does not trust
the workdir or its canonical form, so most launches write nothing. The
write keeps every other key and value and is atomic: a temporary file in
the same directory, with the store's mode, renamed over the store. horch
reads the store again just before the rename and starts again, at most 3
times, when another process changed it. After the write it reads the store
again and fails when it does not trust the workdir.

## Consequences

- It never trusts `/`, the home directory or an ancestor of it, a relative
  path, a path that is not valid UTF-8 (the store would name a different
  folder), or a path that is not an existing directory. It never writes
  through a symlinked store, never changes an explicit Codex
  `trust_level = "untrusted"`, and never creates a Claude or Antigravity
  store: those return `TrustWrite::Skipped`.
- `prepare_launch` in `crates/horch-core/src/harness/launch.rs` writes the
  trust before the adapter's `prepare`: a Codex private home links the
  operator's `config.toml` as it is at prepare time, so trust written later
  would be missing from it.
- For every harness horch holds the lock directory `<store>.lock` across the
  read and the rename. `StoreLock` makes it exactly as proper-lockfile does,
  which is Claude's own lock on `.claude.json`, so horch's write and
  Claude's writes exclude each other. Dropping the guard removes the
  directory. A lock older than `LOCK_STALE` (10 s, proper-lockfile's
  default, which Claude uses) is stale.
- For Codex and Antigravity the lock only serializes horch with horch.
  They write their stores without a lock, so a native write that lands
  between horch's last read and its rename is lost. 2 Codex sessions have
  the same exposure to each other.
- The temporary file is `<store>.horch-tmp.<pid>.<12 hex>`. A process that
  dies before the rename leaves it, so while it holds the lock horch
  removes such a file of a dead pid that is older than 60 s. The tag
  `TEMP_TAG` is `horch-tmp`, different from Claude's own `.tmp.`, so horch
  never removes Claude's files.
- `TempStore::write` calls `set_permissions` after `open`, because the
  umask can narrow the mode `open` asked for. The renamed store keeps the
  original store's mode (0600 for a new store).
- Claude's store is `$CLAUDE_CONFIG_DIR/.claude.json` when the variable is
  set, else `<home>/.claude.json` (`claude_config_file`). Seen on claude
  2.1.289: with the variable set, it writes `.claude.json` in that
  directory and leaves the home one alone. Codex's store is under
  `$CODEX_HOME` when it is set. agy keeps its store under the home
  directory.
- Trust is exact-path. Claude trusts a root when
  `projects["<root>"].hasTrustDialogAccepted` is `true` for the root or its
  canonical form (`claude_trust`). A parent folder's entry does not count:
  Claude keys trust on the repository root. Antigravity trusts a root when
  the list `trustedWorkspaces` in `~/.gemini/antigravity-cli/settings.json`
  holds it (`antigravity_trust`). A parent folder or a subdirectory does not
  count. Seen on agy 1.2.17 (`docs/live-checks/harnesses.md`):
  `{"trustedWorkspaces": ["<dir>", ...]}`.
- `claude_add_trust` and `antigravity_add_trust` edit the JSON text: horch
  inserts members or replaces 1 value and never re-serializes the file, so
  no other value can change (a re-parsed float could). An unexpected shape
  is an error that says why horch cannot edit it safely.
- `codex_add_trust` only appends a `[projects."<key>"]` table with
  `trust_level = "trusted"` for each key, and the text before it stays byte
  for byte. It is an error when a table for a key exists already: an
  explicit `trust_level = "untrusted"` is the operator's choice, and any
  other table horch does not edit. It is an error too when horch does not
  fully understand the file, as an append could then make `config.toml`
  invalid (a duplicate table): the file has a multi-line string, or a line
  that can define `projects` other than a strict `[projects."<path>"]`
  header (`toml_projects_line`). The result has exactly 1 strict header for
  each key.
- `toml_quoted` reads a TOML basic (`"..."`) or literal (`'...'`) string. A
  basic string may use only the escapes `\\` and `\"`. Any other escape
  gives `None`, so horch never takes a spelling it does not decode for the
  path it names.
- The stores can hold tokens. `read_trust` keeps only the verdict, and
  nothing else of a store leaves it. Reasons (`HarnessTrust::reason`,
  `TrustWrite::Skipped`, `trust_note`) name the store file, never any of
  its content.
