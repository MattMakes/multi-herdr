# Shared context for the env-var efficiency research (read this first)

Date: 2026-09-18. Project: /Users/mascott/projects/multi-herdr (the `horch` fleet tool).

## Goal of the overall effort
Find environment variables and settings, per agent harness, that make fleet
workers MORE ACCURATE and CHEAPER IN TOKENS. Efficiency must not cost quality.
The orchestrator will synthesize your findings into `ai_docs/plans_to_improve.md`.
You write a REPORT ONLY. You do not edit `teammates/*.md`, settings files, or
any Rust. Planning phase only.

## Harnesses and models the fleet actually launches (from `horch teammates`)
| harness  | binary     | models in use                                                           |
|----------|------------|-------------------------------------------------------------------------|
| claude   | `claude`   | `opus` (most specialists, opus tier), `sonnet` (sonnet tier, qa-engineer). Orchestrator runs `fable` (claude-fable-5-1). |
| codex    | `codex`    | `gpt-5.6-sol`, `gpt-5.6-terra`                                          |
| opencode | `opencode` | `opencode/nemotron-3.5-lightning-free`, `opencode/big-pickle`, `opencode/nemotron-3-ultra-free` |
| pi       | `pi`       | `ollama/qwen3.8` (local via Ollama)                                     |
| prime    | `prime`    | `anthropic/claude-opus-5` (Prime Agent is a fork of pi, one Python kernel tool) |

Installed Claude Code: 2.1.274 at `/Users/mascott/.local/share/claude/versions/2.1.274`.
Get the other versions yourself with `<bin> --version | head -1` (do NOT run
`prime --version` or `pi --version` without `| head -c 200`; one of them dumps a
JS bundle to stdout). Record versions in your report header.

## What horch ALREADY sets (do not re-recommend these; you may recommend tuning them)
From `crates/horch-core/src/launch.rs` and `crates/horch-core/src/skills.rs`:
- Per-teammate `effort` -> claude `--effort`, codex `-c model_reasoning_effort`, opencode `--variant`, pi/prime `--thinking`.
- claude: `CLAUDE_CODE_SUBAGENT_MODEL` (from teammate `subagent_model`), `--setting-sources`, `--strict-mcp-config`, `--disable-slash-commands`, `--tools/--allowedTools/--disallowedTools`, `--permission-mode`, plugin disabling via a `--settings` overlay, `--plugin-dir`.
- codex: a private `CODEX_HOME` per pane holding only that launch's rules and phase skills.
- opencode: `OPENCODE_CONFIG_CONTENT` (JSON merged in by horch for skills), `--pure` when `inherit_plugins: false`, `--session`.
- pi/prime: `--no-extensions --no-skills --no-prompt-templates --no-themes` when `inherit_plugins: false`, `--tools`, `--exclude-tools` (pi only), `--session-id` (pi) / `--session-dir` (prime).
- Every teammate file supports an `env:` map (key -> value) exported before the CLI starts. THIS is where per-worker env-var recommendations would land. The orchestrator's own pane is started by `horch fleet` and inherits the operator's shell.

The operator's `~/.claude/settings.json` already has (do not re-recommend):
`disableBundledSkills: true`, `disableWorkflows: true`, `enableWorkflows: false`,
`skillListingBudgetFraction: 0.5`, `autoCompactEnabled: true`, `effortLevel: medium`,
`alwaysThinkingEnabled: true`, `disableClaudeAiConnectors: true`, `disableArtifact: true`,
`awaySummaryEnabled: false`, `includeCoAuthoredBy: false`, a `modelSettings` dict, hooks, a statusLine.
There is NO `env` block in settings.json today.

## Verification rule (non-negotiable)
A variable or setting goes in your TOP list only if you verified it exists in
the INSTALLED version, by at least one of:
1. It appears in the binary: e.g.
   `strings <path-to-binary> | grep -oE '(CLAUDE|ANTHROPIC|MAX_|DISABLE|BASH_|MCP_|CODEX|OPENCODE|OLLAMA|PI_)[A-Z0-9_]+' | sort -u`
   (for JS bundles, grep the bundle file; find it via `which <bin>` and follow symlinks with `readlink -f`).
2. It appears in official documentation for that version (cite the URL).
3. `<bin> --help` or `<bin> config --help` lists it.
Anything you recall from memory but could not verify goes in a separate
"Unverified / folklore" section, never in the top list. Say which method
verified each item.

## For EVERY candidate, report these fields
- name (exact spelling), harness, and what it controls
- verified-by (method 1/2/3 above + the evidence line or URL)
- default value in the installed version
- recommended value for a fleet worker, and for the orchestrator if different
- where it lives: teammate `env:` map | settings.json | `~/.codex/config.toml` | `OPENCODE_CONFIG_CONTENT` | opencode.json | Ollama env / Modelfile | pi settings
- expected effect on token usage (rough: small/medium/large, and on which side: input context, output, tool output)
- expected effect on accuracy/quality: helps / neutral / risks X
- your verdict: RECOMMEND / TUNE CAREFULLY / SKIP (with the reason)

## Things that are SKIP by policy (still list them, with reason)
- anything that disables prompt caching or thinking
- anything that truncates model context silently (e.g. tiny `num_ctx` on Ollama)
- anything that sends proprietary code to a training endpoint
- anything that removes the operator's tuned settings wholesale

## Hygiene
If you dump `env`, pipe through `sed -E 's/(KEY|TOKEN|SECRET)=.*/\1=<redacted>/'`.
Never write a credential into any file.

## Output
Write your report to the path named in your brief under `ai_docs/reports/env-research/`.
Reply to the orchestrator with ONLY the file path and a one-line count
("N verified, M unverified, K skipped"). Do not paste the report into the terminal.
