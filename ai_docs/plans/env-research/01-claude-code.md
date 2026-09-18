# Brief: Claude Code env vars and settings for fleet efficiency

Read `/Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/00-shared-context.md` FIRST.
Output: `/Users/mascott/projects/multi-herdr/ai_docs/reports/env-research/claude-code.md`

## Scope
Claude Code CLI version 2.1.274 (binary/bundle at
`/Users/mascott/.local/share/claude/versions/2.1.274`). This is the harness for the
opus and sonnet tiers, all eight specialists, AND the Fable orchestrator, so it
has the highest stakes. Cover BOTH environment variables and `settings.json`
keys (settings keys can be overlaid per teammate via `--settings`, env vars via
the teammate `env:` map).

## Steps
1. Enumerate every env var the installed build reads:
   `strings /Users/mascott/.local/share/claude/versions/2.1.274 | grep -oE '(CLAUDE|ANTHROPIC|MAX_|DISABLE|BASH_|MCP_|USE_|ENABLE_)[A-Z0-9_]+' | sort -u > /tmp/claude-envvars.txt`
   If that path is a directory, find the actual executable inside it (`ls -la`, `file`). Save the list; attach the count to your report.
2. Enumerate settings keys the same way (grep for likely camelCase keys such as
   `autoCompact`, `Budget`, `maxOutput`, `Truncat`, `contextWindow`, `cache`, `thinking`, `effort`).
3. Cross-check against the official docs:
   https://docs.claude.com/en/docs/claude-code/settings (env var table and settings keys),
   https://docs.claude.com/en/docs/claude-code/costs , and the changelog
   https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md for anything added or renamed around 2.1.x.
4. For each candidate below, and any others you find, fill the fields from the shared context. Candidates to investigate explicitly (verify spelling; some may not exist in 2.1.274):
   - `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (compaction threshold - report the DEFAULT threshold and window in this version; the compaction benchmark thread will pick the value, you just report the lever and its default)
   - `BASH_MAX_OUTPUT_LENGTH`, `BASH_DEFAULT_TIMEOUT_MS`, `BASH_MAX_TIMEOUT_MS` (a bash tool result dumped a huge JS bundle into the orchestrator's context today; a cap is the fix)
   - `MAX_MCP_OUTPUT_TOKENS`, `MCP_TIMEOUT`, `MCP_TOOL_TIMEOUT`
   - `CLAUDE_CODE_MAX_OUTPUT_TOKENS`, `MAX_THINKING_TOKENS`
   - `DISABLE_TELEMETRY`, `DISABLE_ERROR_REPORTING`, `DISABLE_AUTOUPDATER`, `DISABLE_NON_ESSENTIAL_MODEL_CALLS`, `DISABLE_COST_WARNINGS`, `DISABLE_PROMPT_CACHING` (policy SKIP, but document why), `DISABLE_INTERLEAVED_THINKING`, `DISABLE_BUG_COMMAND`, `DISABLE_FEEDBACK_COMMAND`, `DISABLE_INSTALLATION_CHECKS`
   - `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`
   - `CLAUDE_CODE_SUBAGENT_MODEL` (already used; note interaction with `Agent` tool)
   - `CLAUDE_CODE_ENABLE_TELEMETRY` / OTEL vars (`OTEL_*`, `CLAUDE_CODE_OTEL_*`) as MEASUREMENT instruments, not savings
   - `CLAUDE_CODE_SKIP_*`, `CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL`, anything `USE_BUILTIN_RIPGREP`
   - settings: `env` block, `cleanupPeriodDays`, `outputStyle`, `skillListingBudgetFraction` (already 0.5; report what lower values cost), `disableAllHooks`, `statusLine` cost, `modelSettings` per-model overrides, `alwaysThinkingEnabled`, `effortLevel`, anything about `compact` instructions in CLAUDE.md (`# Compact instructions`), `includeCoAuthoredBy`
   - the `--effort` levels and what `xhigh` costs vs `high` on Opus 5 (docs), since most teammates run `xhigh`
5. Note which of these apply differently to the orchestrator pane (long-lived, context-precious) versus a short-lived worker.
6. Produce: a ranked TOP 10 table, an "Unverified / folklore" list, and a SKIP list with reasons.

## Out of scope
Do not research other harnesses. Do not edit any file except your report. Do not
change settings.json. Do not research benchmark papers (another worker does).

## Done looks like
The report file exists with: version header, the raw env-var count from step 1,
a top-10 table with every field from the shared context filled, unverified list,
skip list, and a short "orchestrator vs worker" note. Then `horch done` with the path.
