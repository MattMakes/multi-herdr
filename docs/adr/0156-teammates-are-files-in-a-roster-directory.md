# Teammates are files in a roster directory

The roster (`crates/horch-core/src/roster/mod.rs`) is 1 markdown file per
team member, read at run time. Teammate names come from the roster
directory, not from a compiled-in enum. `horch spawn`'s argv parsing
(`crates/horch/src/cmd/spawn.rs`) accepts any word, and the roster rejects
an unknown name with the list of teammates that actually exist on this
machine, which a compiled-in enum could not give. Layers, lowest
precedence first: compiled-in via `crates/horch-core/build.rs`,
`~/.config/horch/teammates` (user-wide additions), then
`$HORCH_TEAMMATES_DIR` (this repo's folder, exported by the justfile, for
tests and one-offs). Overlay is by whole entry, keyed on name.

## Consequences

- `build.rs` bakes the `teammates/` folder into the binary. `horch install`
  is a plain binary copy, so a fleet must be spawnable with no repo checked
  out. The compiled-in copies are the fallback layer, and a runtime
  directory overlays them when one is present.
- The build script's file list is globbed, not hand-written. "Adding a
  file to the folder adds a teammate" then holds for the compiled binary
  too, and a file can never be added on disk yet forgotten in Rust.
- The build script reads `CARGO_MANIFEST_DIR` at run time with
  `std::env::var`, not with `env!`. Cargo reuses a compiled build script
  across checkouts at different paths, because path dependencies hash
  without their absolute path. An `env!` value would point every later
  build at the checkout that first compiled the script.
- The built-ins must always parse, and a test pins it.
- `<cwd>/teammates` is deliberately not searched. A worker's cwd is the
  target project, and a stray folder there would silently re-brief the
  fleet.
- `Brief::resolved` (`crates/horch-core/src/messaging/brief.rs`) carries the
  teammate resolved at spawn time, so the worker renders the briefing the
  spawner actually chose. A worker's cwd is the target project, not the
  horch repo, so re-reading the roster in the pane could resolve a
  different file, or none at all.
- `Roster::overlay` (`crates/horch-core/src/roster/repository.rs`) never
  fails the load on a teammate or base file that does not read or parse. An
  overlay can be ahead of the binary (a new field value), and 1 such file
  must not take every other teammate down. The file is recorded instead,
  the earlier definition of its name stays, `Roster::load_warnings` names
  the file and the error, and `Roster::check` (`--check`) fails on it.
- Only an unreadable directory, or an unreadable `system_prompt/` file
  (plain text, so never ahead of the binary), fails the load.
- Nothing in the roster reads the process environment: the home directory
  and the roster override arrive as parameters.
- `horch teammates new` (`crates/horch/src/cmd/teammatescmd.rs`) scaffolds
  a teammate file by copying `roster::TEMPLATE` (`_template.md`). The
  template is also the schema's documentation, so there is no second copy
  of the field list to drift. A test asserts that the template documents
  exactly the `Teammate` fields and loads under `deny_unknown_fields`.
- The submodules: `teammate` (frontmatter types and orchestrator-only
  constants), `phase` (`Phase`), `effort` (per-agent effort checks),
  `permission` (`PermissionMode`), `offer` (`offer_when`: which teammates a
  project is offered), `parser` (frontmatter splitting and parsing),
  `repository` (`Roster`: built-ins and overlays), `validation` (`--check`:
  `Roster::check` and the fallback rules) and `operator` (the operator's
  `~/.claude/settings.json` and effort overrides).
