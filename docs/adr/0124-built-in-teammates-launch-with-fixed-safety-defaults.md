# Built-in teammates launch with fixed safety defaults

The launch builder sets some options of built-in teammates itself, and an
operator setting does not override them. These options keep commits clean,
keep sandboxed teammates working without prompts, and keep a pipeline off a
harness where it cannot run. `crates/horch-core/tests/harness_defaults.rs`,
`tests/intake_roster.rs` and `tests/video_pipeline_wave.rs` pin them.

## Consequences

- Operator request 2026-11-04: every built-in Claude teammate, orchestrator
  and fleet worker, launches with commit and PR attribution off. The
  default is forced, so an operator `attribution` text does not win.
- On Linux a sandboxed Claude teammate refuses to launch unless `bwrap` and
  `socat` are on PATH. The tests stub both, so the command builder runs on
  every host.
- T-31: each intake teammate's own sandbox `allowRead` and `allowWrite`
  paths, with `~` expanded, are Claude working directories, so a Bash
  command that names one does not stop at a permission prompt. No other
  path is a working directory, so the Read tool reaches no more than Bash.
  An `allowRead`-only path gets an `Edit(...)` deny rule. skill-importer
  keeps its quarantine Read deny.
- No `video-*` seat may fall back to Codex. Chrome, ffmpeg and macOS `say`
  are not live-checked inside the Codex sandbox, so the pipeline would fail
  there.
