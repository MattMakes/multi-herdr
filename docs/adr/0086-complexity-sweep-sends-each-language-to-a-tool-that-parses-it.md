# The complexity sweep sends each language to a tool that parses it

The complexity-sweep finish step (`taskledger::finish::complexity`) sends a
changed file only to a tool that can parse it. `complexity::EXTENSIONS` lists
what the analyser parses: TypeScript and JavaScript (its core is `ts-morph`),
and Python. The analyser has no Rust or GDScript parser: it reads those files
as TypeScript and reports functions that do not exist, so they are not sent
to it (#204). Rust goes to clippy. GDScript goes to
`scripts/godot/gdcomplexity.py`, which parses it with `gdtoolkit.parser`.

When no changed file is in one of these languages, the step is not
applicable with the note "no changed file is in a language that the analyser
(TypeScript, JavaScript, Python), clippy (Rust) or gdcomplexity (GDScript)
reads". A changed file in a routed language is never skipped in silence: a
missing tool fails the step.

## Consequences

- `complexity::rust_crates` groups each changed `.rs` file by its
  nearest `Cargo.toml`, from the file's folder up to the work dir. A file with no
  `Cargo.toml` at or under the work dir is not analysed. So a gdext crate in
  `rust/` of a Godot project is analysed, and clippy runs once per crate in
  that crate's folder. The first crate writes `<seq>-clippy.json`, the next
  ones `<seq>-clippy-<n>.json`.
- `complexity::clippy` runs `cargo clippy` with 3 lints only, capped at warn,
  so a `#![deny(...)]` in the code does not stop the build and hide the
  findings: `clippy::cognitive_complexity` (metric `cognitiveComplexity`),
  `clippy::too_many_lines` (`functionLength`) and
  `clippy::too_many_arguments` (`parameterCount`).
- `complexity::clippy_violations` reads the value and the threshold from the
  message text, "... (26/25)", so the threshold is the one clippy used:
  `clippy.toml` when present, else its default. It takes the function name
  after `fn` when the span is a whole signature. It reports a finding once
  per file, line and metric: a file in 2 targets is reported twice by
  clippy, and 2 lints on 1 function are 2 findings. Paths are made relative
  to the work dir through the crate's `cargo metadata` workspace root.
- `gdcomplexity.py` reports, for each `func_def` (inner-class methods too),
  `cyclomaticComplexity` (1 plus each branch node, lambdas counted in their
  enclosing function) and `parameterCount`, with exact 1-based start and end
  lines. The defaults are 10 and 7. It prints the analyser's JSON shape, so
  `complexity::in_changed_functions` judges it, and it adds an empty-named
  entry after a function's last line so a change below the function is not
  counted in it.
- horch embeds `gdcomplexity.py` with `include_str!`. `finish::Tools`
  takes it from `HORCH_GDCOMPLEXITY_CLI`, else from the embedded copy that
  horch writes to `<evidence_root>/bin/gdcomplexity.py`. The installed horch
  never reads `scripts/godot/` of a checkout.
- The step runs `gdcomplexity.py` through `uv run --no-project --with
  gdtoolkit==4.5.0 python3 -I` when `uv` runs, else through `python3` when it
  imports `gdtoolkit.parser`. With neither, a changed `.gd` file fails the
  step with "no gdscript analyser" and the install command
  `uv tool install gdtoolkit==4.5.0` or `pipx install "gdtoolkit==4.*"`.
- The analyser reads a config only from its current directory. So
  `complexity::config` finds, for each changed file, the nearest
  `static-analysis` config from the file's folder up to the work dir (T-35),
  and the step runs the analyser once per config with `--config`.
  `gdcomplexity.py` takes the same `--config` and reads the
  `cyclomaticComplexity` and `parameterCount` thresholds of a JSON or YAML
  config.
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
