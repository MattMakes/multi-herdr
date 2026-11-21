# Antigravity workers stay on the operator's Google login without skills

An Antigravity (`agy`) worker always runs on the operator's subscription
login. An Antigravity launch is
`agy --model <slug> --effort <level> --prompt-interactive "<prompt>"`
(`crates/horch-core/src/harness/antigravity.rs`). An `agy` child never
receives the variables in `ANTIGRAVITY_FORBIDDEN_ENV`, on top of
`FORBIDDEN_ENV` in `crates/horch-core/src/harness/launch.rs`, even when the
worker's own environment holds them. Each one moves `agy` off the
operator's Google login: `GEMINI_API_KEY` (read when `modelProvider` is
`gemini`), `GOOGLE_API_KEY` (the Gemini tooling's other key name) and
`GOOGLE_GEMINI_BASE_URL` (sends model calls to another endpoint). Only
`agy` children lose them, because another harness or a tool may need them.

## Consequences

- The `agy` command removes these keys with `env_remove`, not just leaves
  them unset. A removed key counts as taken, so the teammate env and the
  child env layers cannot add it back.
- The e2e tests in `crates/horch-e2e/tests/lifecycle.rs` check that these
  keys, and `ANTHROPIC_API_KEY`, are absent from an `agy` child's
  environment.
- `agy` has no way to be pointed at a skills directory, so horch exposes no
  skills to it. An `agy` teammate that names skills is refused before any
  pane exists, instead of launching a worker that silently lacks them.
- `agy` has no positional prompt. `--prompt-interactive` runs the opening
  prompt and stays in the TUI, while `-p` would exit after it. Beside
  `--conversation` it runs as a new turn of that conversation (agy 1.3.0,
  `agy-resume-prompt` in `scripts/live/harnesses.sh`), so a resume keeps
  the prompt. OpenCode ignores the same pair.
- `agy models` lists model ids with an effort level appended
  (`EFFORT_SUFFIXES`: `-low`, `-medium`, `-high`, as in
  `gemini-3.8-flash-low`). An id with one runs without `--effort`, and a
  bare id needs `--effort`.
