# The complexity sweep sends each language to a tool that parses it

The complexity-sweep finish step (`taskledger::finish::complexity`) sends a
changed file only to a tool that can parse it. `complexity::EXTENSIONS` lists
what the analyser parses: TypeScript and JavaScript (its core is `ts-morph`),
and Python. The analyser has no Rust or GDScript parser: it reads those files
as TypeScript and reports functions that do not exist, so they are not sent
to it (#204). Rust goes to clippy's `cognitive_complexity` lint instead.

## Consequences

- `complexity::clippy` runs `cargo clippy` with only the cognitive-complexity
  lint, capped at warn, so a `#![deny(...)]` in the code does not stop the
  build and hide the findings.
- `complexity::clippy_violations` reports each `cognitive_complexity` finding
  of `cargo clippy --message-format=json` once: a file in 2 targets is
  reported twice. The value and the threshold come from the message text,
  "... of (26/25)", so the threshold is the one clippy used: `clippy.toml`
  when present, else its default.
- The analyser reads a config only from its current directory. So
  `complexity::config` finds, for each changed file, the nearest
  `static-analysis` config from the file's folder up to the work dir (T-35),
  and the step runs the analyser once per config with `--config`.
- `config::SEARCH_PLACES` mirrors the file names cosmiconfig 9 (the
  analyser's config loader) searches in 1 folder, in its order, for the
  module name `static-analysis`. `package.json` counts only when it has a
  `static-analysis` field. The list must follow cosmiconfig if the analyser
  upgrades.
- `Analyser::run` sends the analyser's stdout to a file, not a pipe: node
  exits before a piped stdout drains, and a pipe keeps only the first 64 KiB
  of the report. The step reads the whole report. The e2e test
  `a_complexity_report_past_64_kib_is_read_whole` in
  `crates/horch-e2e/tests/taskledger.rs` puts the violation past 64 KiB to
  prove it.
