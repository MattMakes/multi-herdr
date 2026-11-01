# Fleet pitfalls live permission check — 2026-10-28

## #58: Claude auto mode and read-only allowedTools

Claude CLI version: 2.1.312. Authentication used the operator's existing claude.ai subscription login. The successful check ran with the runtime permissions needed to access that login, in a scratch Git repository at `/private/tmp/fleet18-permission-probe`.

Command form:

```sh
env -u ANTHROPIC_API_KEY claude -p '<Run only git status --short; report READ_ONLY_CHECK_OK after it succeeds>' \
  --model haiku --permission-mode auto --allowedTools 'Bash(git status *)' \
  --max-turns 2 --output-format json
```

Observed: process exit 0; `is_error: false`; subtype `success`; terminal reason `completed`; 2 turns; `permission_denials: []`; result `READ_ONLY_CHECK_OK`, followed by a clean working-tree report. The agent explicitly declined an unrelated file-writing/subprocess snippet and ran only the requested status check. The root agent inspected only non-secret result fields. Raw local result: `/private/tmp/fleet18-permission-probe/result.json`.

An earlier probe inside the restricted execution environment exited 1 with `Not logged in · Please run /login`, zero input/output tokens and no permission decision. That result did not prove whether the allow list was honored; the successful subscription probe supersedes it.

This verifies the listed git-status command under auto mode. It does not establish that every possible pattern or command is safe or supported. The shipped Claude worker lists use native Read/Grep/Glob, exact read-only Git and cloud command forms, and ledger read command patterns. They introduce no write, push or install command. Shell writes and arbitrary commands remain subject to Claude's permission handling.

All Claude fleet-worker teammates receive the same list. Existing launch oracles are changed only by the `--allowedTools` argument pair, for exactly the changed teammates already represented by an oracle. Removing that pair restores each original JSON document exactly. The teammate and oracle names and the complete list are recorded in `ai_docs/checkpoints/T3-allowedtools-summary.json`.

## Current Codex capabilities

The v0.4.0 `fleet_write` launch profile explicitly permits project and `.git` writes plus network access. Other filesystem locations remain sandboxed, and plan mode is read-only. The historical issue's no-network/no-.git wording predates this configuration; descriptions state the current launch behavior. No launch permission was weakened or expanded for this issue.
