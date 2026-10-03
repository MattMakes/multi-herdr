# D08 research: the Antigravity terminal agent

Date: 2026-10-03. Author: opus-30.

## Answer

A terminal agent exists. It is **Antigravity CLI**, binary `agy`, a single Go
binary that shares its agent core with the Antigravity 2.0 desktop app. Google
replaced Gemini CLI with it for individual developers in June 2026. The right
target is `agy`, not `gemini` and not the app's `antigravity` script.

Recommendation: implement `HarnessKind::Antigravity` on `agy`. Default binary
`agy`, override `HORCH_ANTIGRAVITY_BIN`.

## Local facts (this Mac)

- `/Applications/Antigravity.app` is version 1.13.3. Its
  `Contents/Resources/app/bin/antigravity` is the VS Code-style editor
  launcher (`antigravity --help` prints `Antigravity 1.104.0`, editor and
  extension flags only, plus `--add-mcp`). It has no agent mode.
- The app bundle has `extensions/antigravity/bin/language_server_macos_arm`.
  This is the IDE's agent backend. It is not a terminal CLI.
- `agy` is **not installed** (`which agy` finds nothing; no `~/.local/bin/agy`).
- `gemini` 0.21.3 is at `/opt/homebrew/bin/gemini`. `~/.gemini/` holds a
  Gemini CLI OAuth login. No `~/.gemini/antigravity-cli/` exists yet.
- I ran only `--help` and `--version`. I started no model turn. I read no
  credential file.

## Facts about `agy` (from the docs)

Install: `curl -fsSL https://antigravity.google/cli/install.sh | bash`. It
installs `~/.local/bin/agy`.

| Topic | Fact |
|---|---|
| Interactive mode | `agy` in a directory starts a TUI. `agy "<prompt>"` is not documented; the prompt goes in a positional or the TUI. |
| Headless | `-p`/`--print`/`--prompt "<text>"`, `--output-format text\|json\|stream-json`, `--print-timeout <dur>` (default 5m). |
| Model | `--model <slug>`, also `/model`. Gemini, Claude and gpt-oss models. |
| Effort | `--effort low\|medium\|high`. |
| Permissions | `--dangerously-skip-permissions` (approve every tool call), `--sandbox`, `--mode accept-edits\|plan`. settings `toolPermission`: `request-review`, `proceed-in-sandbox`, `always-proceed`, `strict`. |
| Workspace | `--add-dir <path>` (repeatable). |
| Session id | CLI-minted UUID. No flag to set the id of a new conversation. |
| Resume | `--conversation <id>`; `-c`/`--continue` resumes the newest one for the cwd. |
| Session cache | `~/.gemini/antigravity-cli/cache/last_conversations.json` maps workspace path to its newest conversation id. |
| Headless JSON | `conversation_id`, `status`, `response`, `usage` (`input_tokens`, `output_tokens`, `thinking_tokens`, `cache_read_tokens`, `total_tokens`). |
| Exit | 0 on success; non-zero with `status: ERROR`. |
| Skills | Global `~/.gemini/antigravity-cli/skills/`; workspace `.agents/skills/<name>/SKILL.md` (or `<name>.md`). No documented skills-dir flag or env var. |
| Rules | `GEMINI.md` and `AGENTS.md` (same places as Gemini CLI); `.agents/rules/*.md`; global `~/.gemini/antigravity-cli/GEMINI.md`. |
| MCP | Global `~/.gemini/config/mcp_config.json`; workspace `.agents/mcp_config.json`. |
| Plugins | `~/.gemini/antigravity-cli/plugins/<name>/plugin.json`. |
| Settings | `~/.gemini/antigravity-cli/settings.json`. |
| Auth | Default: Google account, browser OAuth, tokens in the OS keyring. Alternative: `modelProvider: "gemini"` in settings plus `GEMINI_API_KEY`. `GOOGLE_GEMINI_BASE_URL` redirects model calls. |
| Usage | `/usage` (`/quota`) TUI panel, per-model quota. No documented non-interactive reader. Headless JSON has per-run tokens. |
| Workspace trust | First launch in a directory asks "Do you trust the contents of this project?". Trust is exact-path in `settings.json` `trustedWorkspaces`. `--dangerously-skip-permissions` does not skip it. No env var skips it (third-party test on agy 1.1.3). |
| Data policy | Personal accounts: interactions can be used for training unless the operator opts out (Activity and Telemetry off). Workspace/GCP accounts: not used for training. |

## Decisions for the adapter

- **Session**: discovered, not caller-minted. Read
  `last_conversations.json` for the pane's workdir after launch. Resume with
  `--conversation <id>`. The cache file has no timestamps; I use its mtime
  against the launch time.
- **Effort**: `low`, `medium`, `high`.
- **Skills**: no flag or env var points `agy` at a skills dir. The only
  native places are the operator's global dir and the workspace
  `.agents/skills/`. Both are shared state. The adapter reports
  `SkillExposure::None`; skills still reach the worker through the brief's
  routing text if horch adds it for None-exposure harnesses (see the report).
- **Forbidden env**: `GEMINI_API_KEY` and `GOOGLE_API_KEY` bypass the
  operator's Google login when a provider setting allows it.
  `GOOGLE_GEMINI_BASE_URL` sends model calls elsewhere. The adapter removes
  all 3 from `agy` children only. Other harnesses keep them (codex, opencode
  and tools a worker runs can need them).
- **Permission**: `permission_mode: bypassPermissions` maps to
  `--dangerously-skip-permissions`; `acceptEdits` maps to
  `--mode accept-edits`; `plan` maps to `--mode plan`.
- **Workspace trust**: horch cannot answer the trust prompt. The operator
  trusts each worktree once, or adds the path to `trustedWorkspaces`. The
  report gives the step. horch does not write the operator's settings.
- **Third-party use**: the Antigravity FAQ says third-party tools cannot use
  Antigravity credentials. horch does not read or use the credentials. It
  runs the operator's own `agy` binary in a terminal pane, the same as a
  human. The operator decides if this is acceptable.

## Sources

- https://antigravity.google/docs/cli/overview/
- https://antigravity.google/docs/cli/install/
- https://antigravity.google/docs/cli/reference/
- https://antigravity.google/docs/cli/headless/
- https://antigravity.google/docs/cli/conversations/
- https://antigravity.google/docs/cli/commands/resume/
- https://antigravity.google/docs/cli/commands/usage/
- https://antigravity.google/docs/cli/features/
- https://antigravity.google/docs/cli/gcli-migration/
- https://antigravity.google/docs/models/
- https://antigravity.google/docs/faq/
- https://computingforgeeks.com/antigravity-cli-cheat-sheet/ (flag table)
- https://www.gradually.ai/en/changelogs/antigravity/ (headless fixes, Sept 2026)
- https://github.com/bmad-code-org/bmad-loop/issues/169 (trust prompt, exact-path trust)
- https://github.com/herdrdev/herdr/issues/3419 (trust prompt shows as idle in herdr)
- https://github.com/google-gemini/gemini-cli/discussions/27274 (Gemini CLI to Antigravity CLI transition)

The flag table comes partly from a third-party cheat sheet. The model slugs on
the models page did not parse reliably. The operator must confirm flags and
slugs with `agy --help` after install.
