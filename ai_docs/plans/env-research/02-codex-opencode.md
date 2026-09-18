# Brief: Codex CLI and OpenCode env vars / config for fleet efficiency

Read `/Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/00-shared-context.md` FIRST.
Output: `/Users/mascott/projects/multi-herdr/ai_docs/reports/env-research/codex-opencode.md`
(two top-level sections, one per harness, each with its own top-10).

## Part A: Codex CLI (`codex`, models gpt-5.6-sol and gpt-5.6-terra)
1. Record `codex --version | head -1`. Find the binary (`readlink -f $(which codex)`).
2. Enumerate: `strings <binary> | grep -oE '(CODEX|OPENAI)[A-Z0-9_]+' | sort -u`, plus `codex --help`, `codex exec --help`, and `codex config --help` if present.
3. Read the official config reference: https://github.com/openai/codex/blob/main/docs/config.md (and `docs/` neighbours: `advanced.md`, `sandbox.md`). Config lives in `$CODEX_HOME/config.toml`; horch gives each pane a PRIVATE `CODEX_HOME` (see `crates/horch-core/src/codex.rs`), so per-worker config.toml keys are a legitimate landing place. Also `-c key=value` CLI overrides (horch already uses `-c model_reasoning_effort`).
4. Candidates to verify: `model_reasoning_effort` levels and cost per level, `model_reasoning_summary`, `model_verbosity`, `model_context_window`, `model_auto_compact_token_limit` (or whatever the compaction key is named in this version - report its DEFAULT), `history.persistence`, `hide_agent_reasoning`, `show_raw_agent_reasoning`, `disable_response_storage`, `tool_output_token_limit`-style caps for shell output, `shell_environment_policy` (inherit vs core, so a worker's env stays small), `notify`, `project_doc_max_bytes` (AGENTS.md size cap), `sandbox_mode`/`approval_policy` (already handled by horch? check `crates/horch-core/src/launch.rs` `codex_command`), `web_search`, `experimental_*`, `CODEX_HOME`, `OPENAI_BASE_URL`, anything for prompt caching or `service_tier`.
5. Fill every field from the shared context. Landing place is either the private config.toml (say so) or a `-c` override in the teammate `args:` list or the teammate `env:` map.

## Part B: OpenCode (`opencode`, free-tier models nemotron-3.5-lightning-free, big-pickle, nemotron-3-ultra-free)
1. Record `opencode --version | head -1`. Find the bundle (`readlink -f $(which opencode)`).
2. Enumerate: `strings <bundle> | grep -oE 'OPENCODE_[A-Z0-9_]+' | sort -u`, plus `opencode --help`, `opencode run --help`.
3. Official docs: https://opencode.ai/docs/config/ , https://opencode.ai/docs/cli/ , https://opencode.ai/docs/models/ , https://opencode.ai/docs/agents/ , and whatever the docs say about compaction / autocompact / `compaction` settings and `OPENCODE_CONFIG_CONTENT`. Note horch already merges an `OPENCODE_CONFIG_CONTENT` JSON object (see `crates/horch-core/src/skills.rs` around line 230-290), so additional JSON keys can be recommended there.
4. Candidates: `OPENCODE_CONFIG`, `OPENCODE_CONFIG_CONTENT`, `OPENCODE_DISABLE_AUTOUPDATE`, `OPENCODE_DISABLE_*`, `OPENCODE_PERMISSION`, autocompact/compaction thresholds and their defaults, `--variant` levels per model (which of the three models support which variants; `opencode models --verbose` or similar), per-model `limit.context`/`limit.output` in provider config, `instructions` file loading (AGENTS.md size), `share: disabled` (data leaves the machine otherwise), `autoupdate`, `snapshot`/`git` features, tool output truncation limits, any "trains on input" opt-out for the free tier (report; the fleet already restricts these tiers to public work).
5. Fill every field. Landing place: `OPENCODE_CONFIG_CONTENT` JSON via teammate `env:`, opencode.json, or teammate `args:`.

## Out of scope
No Claude Code, no pi/Ollama/Prime, no benchmark papers. No file edits except the report.

## Done looks like
Report with two version headers, two raw enumeration counts, two top-10 tables
with all fields, two unverified lists, two skip lists. Then `horch done` with the path.
