# D08 antigravity-harness: research and adopt the Antigravity CLI agent

Unit slug: `antigravity-harness`. Branch: `ds/antigravity-harness`.

## GOAL

horch can spawn workers on Google's Antigravity agent from the terminal,
exactly like the other harnesses: a `HarnessKind` adapter with honest
capabilities, a binary override, launch argv, session id handling, skill
exposure, a usage pool, at least 1 roster teammate, and hermetic e2e tests
with a fake binary. If no terminal agent exists, the unit stops after the
research with a clear report and options.

## CONTEXT

- Read first: `00-conventions.md`. Then the harness layer:
  `crates/horch-core/src/harness/` (`mod.rs` `HarnessKind`, `Capabilities`
  in `capabilities.rs`, one adapter per harness: `claude.rs`, `codex.rs`,
  `opencode.rs`, `pi.rs`, `prime.rs`; `launch.rs`), the runtime bins
  (`crates/horch-core/src/runtime/`: `HarnessBins`, the `HORCH_*_BIN`
  overrides), routing pools (`crates/horch-core/src/routing/quota.rs`,
  `quota_probe.rs`, `policy.rs`), roster validation
  (`crates/horch-core/src/roster/`), fakes
  (`crates/horch-e2e/src/bin/fake-*.rs`), and the e2e tests
  `crates/horch-e2e/tests/lifecycle.rs` (`arc_26_e2e_lifecycle_matrix_*`)
  and `skills_exposure.rs` (`skl_06_e2e_exposure_*`). Reports:
  `ai_docs/reports/arch-refactor-dataset/a4-harness.md`, `a10-exposure.md`,
  `a6b-service.md`, `session-discovery-race.md`.
- Facts on this machine: `/Applications/Antigravity.app` exists with
  `Contents/Resources/app/bin/antigravity` (maybe only an editor launcher).
  `gemini` (Gemini CLI) is at `/opt/homebrew/bin/gemini`. `~/.gemini/`
  holds an OAuth login. Do not print or copy any credential file.
- Research (network allowed for this step only; use web search and fetch,
  and the local app bundle): Is there an Antigravity agent CLI (name,
  install, binary)? Its interactive mode, model selection, effort or
  thinking levels, session ids and resume, skills or rules format, MCP
  config, settings files, auth (it must use the operator's Google login, no
  API key), transcripts or usage data for `horch cost`, exit behavior,
  workspace-trust prompts. Run only `--help`, `--version` or equivalent
  commands locally. Do not start a model turn without asking first.
- Decision point: send `QUESTION:` with your findings and a recommendation
  before you implement if (a) no Antigravity terminal agent exists, or
  (b) the right target is a different binary (for example Gemini CLI).
- `ANTHROPIC_API_KEY` stays forbidden. Find out whether this harness has an
  equivalent key variable (for example `GEMINI_API_KEY`,
  `GOOGLE_API_KEY`) that would bypass the subscription login; if so, add it
  to `FORBIDDEN_ENV` for this harness's children and say so in the report.
- Parallel unit: D09 `agent-list` lists harnesses from `HarnessKind`; it
  iterates all kinds, so your new kind appears there after both merge.

## FILES

own:
- `crates/horch-core/src/harness/antigravity.rs` (new), `harness/mod.rs`,
  `harness/capabilities.rs`, `harness/launch.rs` (only where a new kind needs it)
- `crates/horch-core/src/runtime/` (the new bin override only)
- `crates/horch-core/src/routing/` (a pool for this harness only)
- `crates/horch-core/src/roster/` (accept the new agent only)
- `crates/horch-e2e/src/bin/fake-antigravity.rs` (new) and `crates/horch-e2e/Cargo.toml` (its `[[bin]]`)
- `crates/horch-e2e/tests/lifecycle.rs`, `skills_exposure.rs` (new tests only)
- `teammates/antigravity.md` (new; more if justified), `teammates/README.md`
  (1 row), `teammates/_template.md` (the `agent:` comment only)
- new oracle files for the new teammate
- `README.md` (the harness list only)
- `ai_docs/reports/design-skills/antigravity-harness.md`

do not touch: other teammates, `skills/`, `crates/horch/src/cmd/` (except
if a match on `HarnessKind` there fails to compile: then the minimal arm).

## STEPS

0. Create the worktree (conventions §3).
1. Research. Write `ai_docs/reports/design-skills/antigravity-research.md`
   with sources (URLs), the facts above, and a recommendation. Commit it.
   Send `NOTE:` (or `QUESTION:` per the decision point) to the orchestrator.
2. Adapter: `HarnessKind::Antigravity` (serde name `antigravity`),
   `Capabilities` (only what the CLI really supports), launch argv, session
   handling (caller-minted or discovered), skill exposure (its native rules
   or skills dir; else document "no skill exposure" in capabilities), binary
   override `HORCH_ANTIGRAVITY_BIN`, usage pool (name it after the provider
   account), roster acceptance.
3. Fake `fake-antigravity` that records argv, cwd and env like the other
   fakes.
4. Teammate `antigravity` (generic worker; brief_description states model,
   strengths, and data policy). Bless its new oracle files.
5. Tests: `arc_26_e2e_lifecycle_matrix_antigravity`,
   `skl_06_e2e_exposure_antigravity` (if it exposes skills), an env test that
   the forbidden keys never reach the child, unit tests for argv.
6. Gate. Commits per step (`Harness: ...`, `Teammates: Add antigravity`).
   Write the report with a local acceptance check for the operator's Mac.
   Follow conventions §7.

## DONE WHEN

- `horch spawn antigravity "..."` works against the fake in e2e; the gate is
  green; the report says exactly what was verified only with the fake.

## REPORT

- Research summary, capabilities table, auth and forbidden-env decision,
  the local acceptance steps for the operator.
