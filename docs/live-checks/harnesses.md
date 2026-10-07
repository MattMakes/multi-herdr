# Live check: harness CLIs

The installed harness CLIs behave as horch's adapters assume: Antigravity
(`agy`) flags, model, resume and trust store; OpenCode's `--prompt` on a
resume; the Prime daemon status record; pi on the installed Node. Items U-28,
U-60, U-61, U-62, U-63, U-64, U-67 and U-30 Q1.

## How to run

```bash
scripts/live/harnesses.sh
AGY_READ_CACHE=1 scripts/live/harnesses.sh   # also read agy's last_conversations.json
```

It needs `agy` signed in to the operator's Google account, `opencode` with
network access to its free tier, `prime-agent` and `pi`. A missing tool is
`SKIP`. The whole run takes about 3 minutes.

Cost: 3 small `agy` turns on the operator's Google login (a 4th is refused
before it starts) and 2 small `opencode` turns on the free model
`opencode/nemotron-3.5-lightning-free`. Both train on input, so every prompt is dummy text ("Reply with the single
word PONG.") in a scratch repo that holds 1 dummy file. The script starts no
`claude` and no Codex session.

The script writes only under `.worktrees/_scratch/live-harnesses/`: the
scratch repos `agy/` and `opencode/` (re-created on each run) and
`opencode-run.log`. It prints only key names, types and counts from
`~/.gemini/antigravity-cli/settings.json`, never the values.
`last_conversations.json` lists the operator's own workspaces, so the script
reads it only with `AGY_READ_CACHE=1`, and then prints only the value type of
the scratch repo's entry.

Warning: running `agy` with another `HOME` (to isolate its state) starts its
self-updater. On 2026-10-06 that run replaced `~/.local/bin/agy` 1.2.17 with
1.3.0. The script never changes `HOME`.

Steps:

- `agy-version`: `agy --version`.
- `agy-flags`: `agy --help` lists the 7 flags the adapter passes
  (`harness/antigravity.rs`).
- `agy-model`: `agy models` lists `gemini-3.8-flash-low|medium|high`, the
  model of `teammates/antigravity.md`.
- `agy-no-api-key`: `settings.json` has no `modelProvider`, so `agy` needs no
  `GEMINI_API_KEY`.
- `agy-trust-store`: `trustedWorkspaces` in `settings.json` is a list of path
  strings, the shape `harness/trust.rs:antigravity_trust` reads (PRE-14).
- `agy-turn`: 1 print-mode turn with the teammate's model and effort. `agy`
  mints the `conversation_id`.
- `agy-resume`: `--conversation <id>` keeps the id and recalls the first turn.
- `agy-mode-plan`: `--mode plan` is accepted.
- `agy-print-no-trust`: print mode runs in an untrusted directory and does not
  add it to `trustedWorkspaces`.
- `agy-effort-refusal`: `--effort` on a model id that `agy models` does not
  list is refused.
- `agy-no-usage-file`: no file with `usage` or `quota` in its name under
  `~/.gemini/antigravity-cli` (U-60, U-61).
- `agy-cache-format`: with `AGY_READ_CACHE=1`, `last_conversations.json` maps
  the scratch repo to a string or an object.
- `agy-fleet-spawn`: always `SKIP`. It needs a fleet and a trusted directory
  (see "For the operator").
- `opencode-resume-prompt`: a session from `opencode run`, then the TUI with
  `--session <id> --prompt <p>` in a pseudo-terminal for 45 s. The session
  gains no user turn. Control: the same TUI setup with only `--prompt` starts
  a session that holds the prompt.
- `prime-status`: the installed `prime-agent` builds its `status --json`
  record from `pid`, `socketPath` and `uptimeSeconds` (`ps -o etimes` of that
  pid), with no start time of its own (`cli/daemon-ps.js`).
- `pi-version`: `pi --version` runs on the default `node`.

## For the operator

These steps of the 11-step acceptance in the Antigravity harness report need
a fleet and a trusted directory, so the script does not run them:

1. Trust the directory once: `cd <worktree> && agy`, choose to trust it, then
   exit.
2. In a running fleet:
   `horch spawn antigravity "Print the repo's top-level files and run horch done."`
3. `horch sessions --json`: the record has a `session_id`.
4. `AGY_READ_CACHE=1 scripts/live/harnesses.sh`: `agy-cache-format` passes.
5. `horch spawn --resume <record_id> "Say what you did before."`: the worker
   recalls the first task.

A worker in auto mode runs `agy --sandbox --dangerously-skip-permissions`.
This check did not run that form: the agent's auto-mode safety check blocked
it. Whether `--sandbox` lets a worker write `.git` is still unverified.

## 2026-10-06

Host: macOS 26 (Darwin 25.5.0), arm64. Runner: opus-10 (W3d).

| Step | Claim it proves | Tool version | Result | Evidence |
|---|---|---|---|---|
| agy-version | U-28: `agy` is installed | agy 1.2.17, then 1.3.0 | PASS | `~/.local/bin/agy`; self-updated to 1.3.0 at 17:35 (see the warning) |
| agy-flags | U-28: `--prompt-interactive`, `--model`, `--effort`, `--mode`, `--sandbox`, `--dangerously-skip-permissions`, `--conversation` exist (`harness/antigravity.rs`) | 1.2.17, 1.3.0 | PASS | all 7 in `agy --help` |
| agy-model | U-28: the slug `gemini-3.8-flash` with `--effort` (`teammates/antigravity.md`) | 1.2.17, 1.3.0 | PASS | `agy models` lists `gemini-3.8-flash-low/medium/high` |
| agy-no-api-key | U-28 step 5: no Gemini key is needed | 1.3.0 | PASS | `settings.json` has only `trustedWorkspaces` |
| agy-trust-store | U-62: trust is the exact-path list `trustedWorkspaces` (`harness/trust.rs:antigravity_trust`) | 1.2.17, 1.3.0 | PASS | a list with 1 path string, the main checkout |
| agy-turn | U-28: the teammate argv runs; `agy` mints the id | 1.2.17, 1.3.0 | PASS | `status: SUCCESS`, a UUID `conversation_id`; 1-line dummy turn |
| agy-resume | U-28 step 11: `--conversation <id>` resumes | 1.2.17, 1.3.0 | PASS | same id; the answer is the remembered word |
| agy-mode-plan | U-28: `permission_mode: plan` maps to `--mode plan` | 1.2.17, 1.3.0 | PASS | `status: SUCCESS`. 1.3.0 needs `--effort` with a bare id |
| agy-print-no-trust | U-62: print mode needs no trust and records none | 1.2.17, 1.3.0 | PASS | the scratch repo is not in `trustedWorkspaces` |
| agy-effort-refusal | `teammates/antigravity.md` comment: `agy` refuses `--effort` for an id it does not list | 1.2.17, 1.3.0 | PASS | "--effort is not supported for model gemini-3-1-pro" |
| agy-no-usage-file | U-60, U-61: no local usage or quota signal (`telemetry/readers.rs`, `routing/quota_probe.rs` comments) | 1.2.17, 1.3.0 | PASS | no such file; state files are `*.pb` and `conversation_summaries.db` (no token column); only `-p --output-format json` prints `usage` |
| agy-cache-format | U-28 step 10: `last_conversations.json` format | 1.3.0 | SKIP | the agent's safety check blocked reading the operator's own agy state; operator runs it with `AGY_READ_CACHE=1` |
| agy-fleet-spawn | U-28 steps 8 to 11 in a fleet | - | SKIP | needs a fleet and a trusted directory; see "For the operator" |
| opencode-resume-prompt | U-64: `--prompt` beside `--session` is ignored, so the resume argv drops it (`harness/opencode.rs`) | opencode 1.18.34, then 1.18.35 | PASS | 1 user turn before and after the resume; the control session holds its prompt; 2 dummy free-tier turns |
| prime-status | U-63: Prime reports no daemon start time (`harness/prime.rs:Daemon::finish` comment) | prime-agent 0.9.4 | PASS | `status --json` printed `[]` (no daemon); the record is `pid`, `socketPath`, `uptimeSeconds` from `ps -o etimes` |
| pi-version | U-30 Q1: pi runs on the installed Node, so no `HORCH_PI_BIN` wrapper is needed | pi 0.99.1, node v24.21.0 | PASS | `pi --version` prints `0.99.1` |

Not covered: `--sandbox` and `--dangerously-skip-permissions` (blocked by the
agent's safety check), and the TUI path of `agy` (the scratch repo is not
trusted).
