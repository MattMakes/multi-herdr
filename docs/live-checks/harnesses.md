# Live check: harness CLIs

The installed harness CLIs behave as horch's adapters assume: Antigravity
(`agy`) flags, model, resume and trust store; OpenCode's `--prompt` on a
resume; the Prime daemon status record; pi on the installed Node. Items U-28,
U-60, U-61, U-62, U-63, U-64, U-67 and U-30 Q1.

## How to run

```bash
scripts/live/harnesses.sh
AGY_READ_CACHE=1 scripts/live/harnesses.sh   # also read agy's last_conversations.json
AGY_TRUST_SCRATCH=1 scripts/live/harnesses.sh  # trust the scratch repo once, for agy-resume-prompt
```

It needs `agy` signed in to the operator's Google account, `opencode` with
network access to its free tier, `prime-agent` and `pi`. A missing tool is
`SKIP`. The whole run takes about 3 minutes.

Cost: 4 small `agy` turns on the operator's Google login (a 5th and a 6th
are refused before they start) and 2 small `opencode` turns on the free model
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

The `agy` TUI asks "Do you trust the contents of this project?" in a new
directory. With `AGY_TRUST_SCRATCH=1` the script presses Enter once at that
question. Then `trustedWorkspaces` holds the scratch repo path
`.worktrees/_scratch/live-harnesses/agy`. Trust is per path, so later runs
need no flag. To remove the trust, delete that path from `trustedWorkspaces`
in `~/.gemini/antigravity-cli/settings.json`.

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
- `agy-resume-prompt`: the TUI with `--conversation <id> --prompt-interactive
  "What is 17 times 23? ..."` in a pseudo-terminal for 45 s shows `391`.
  Control: the same resume without a prompt, for 30 s, shows the first turn
  and the 17 x 23 turn in the history, so the turn is in `<id>`. It needs a
  trusted scratch repo (`AGY_TRUST_SCRATCH=1`), else `SKIP`.
- `agy-print-no-trust`: print mode runs in an untrusted directory and does not
  add it to `trustedWorkspaces`.
- `agy-effort-refusal`: `--effort` on a model id that `agy models` does not
  list is refused.
- `agy-bare-model`: `agy` 1.4.0 accepts `--model gemini-3.8-flash` without
  `--effort` (1.3.0 refused it with "requires --effort"). `PASS` when `agy`
  accepts the bare id. `horch teammates --check` still fails an agy teammate
  with a bare model id and no `effort:` (`roster/validation.rs`): horch
  updates every CLI before each fleet, and a CLI's default effort can change
  between versions, so every agy teammate states its effort. This step was
  `agy-bare-needs-effort` until 1.4.0; the rows below keep that name.
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
- `prime-status`: the Rust build (0.10.0). A scratch Prime TUI (no prompt, no
  model turn) runs on the socket `<scratch>/prime-agent-<uid>/d.sock`.
  `TMPDIR=<scratch>/ prime-agent status --json` names that socket with `pid`
  and `socketPath` and no start time of its own (U-63), and
  `TMPDIR=<scratch>/ prime-agent shutdown --force --json` names it in
  `stopped` with an empty `failed`. This is what `Daemon::finish` relies on.
  Runs before 2026-10-17 checked the TypeScript build's `cli/daemon-ps.js`.
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
| agy-resume-prompt | W13 C6: the resume argv `--conversation <id> --prompt-interactive <p>` delivers `<p>` (`harness/antigravity.rs`) | 1.3.0 | PASS | run by opus-17 (W13): see below |
| agy-print-no-trust | U-62: print mode needs no trust and records none | 1.2.17, 1.3.0 | PASS | the scratch repo is not in `trustedWorkspaces` |
| agy-bare-needs-effort | W13 C6: a bare id needs `--effort` (`roster/validation.rs`) | 1.3.0 | PASS | run by opus-17 (W13): see below |
| agy-effort-refusal | `teammates/antigravity.md` comment: `agy` refuses `--effort` for an id it does not list | 1.2.17, 1.3.0 | PASS | "--effort is not supported for model gemini-3-1-pro" |
| agy-no-usage-file | U-60, U-61: no local usage or quota signal (`telemetry/readers.rs`, `routing/quota_probe.rs` comments) | 1.2.17, 1.3.0 | PASS | no such file; state files are `*.pb` and `conversation_summaries.db` (no token column); only `-p --output-format json` prints `usage` |
| agy-cache-format | U-28 step 10: `last_conversations.json` format | 1.3.0 | SKIP | the agent's safety check blocked reading the operator's own agy state; operator runs it with `AGY_READ_CACHE=1` |
| agy-fleet-spawn | U-28 steps 8 to 11 in a fleet | - | SKIP | needs a fleet and a trusted directory; see "For the operator" |
| opencode-resume-prompt | U-64: `--prompt` beside `--session` is ignored, so the resume argv drops it (`harness/opencode.rs`) | opencode 1.18.34, then 1.18.35 | PASS | 1 user turn before and after the resume; the control session holds its prompt; 2 dummy free-tier turns |
| prime-status | U-63: Prime reports no daemon start time (`harness/prime.rs:Daemon::finish` comment) | prime-agent 0.9.4 | PASS | `status --json` printed `[]` (no daemon); the record is `pid`, `socketPath`, `uptimeSeconds` from `ps -o etimes` |
| pi-version | U-30 Q1: pi runs on the installed Node, so no `HORCH_PI_BIN` wrapper is needed | pi 0.99.1, node v24.21.0 | PASS | `pi --version` prints `0.99.1` |

Not covered: `--sandbox` and `--dangerously-skip-permissions` (blocked by the
agent's safety check).

### W13 C6 run (opus-17)

`AGY_TRUST_SCRATCH=1 scripts/live/harnesses.sh`, agy 1.3.0: 17 PASS, 2 SKIP
(`agy-cache-format`, `agy-fleet-spawn`), 0 FAIL.

- `agy-bare-needs-effort` (now `agy-bare-model`; agy 1.3.0 only): agy printed
  "--model gemini-3.8-flash requires
  --effort (available: low, medium, high)" and ran no turn. An id with the
  level in it, `--model gemini-3.8-flash-low` without `--effort`, ran 1 turn
  with `status: SUCCESS`.
- `agy-resume-prompt`: before the script step, a manual run in
  `.worktrees/_scratch/c6-agy` gave the same result. A print turn minted the
  id. The resumed TUI showed the first turn (`ZEBRA`, `PONG`), then the new
  prompt, then `391`. The control resume showed both turns and ran no new
  turn. A print-mode resume of the same id returned the same
  `conversation_id`, `num_turns: 3`, and listed "What is 17 times 23?".
- Decision: `agy` delivers `--prompt-interactive` beside `--conversation`.
  The resume argv keeps it, and horch types no second copy. This is the
  opposite of OpenCode (`opencode-resume-prompt`).
- A 2nd run without the flag: `agy-resume-prompt` PASS, and
  `agy-print-no-trust` SKIP, because the path was trusted before the run.
- The TUI path of `agy` now runs in the trusted scratch repo. The fleet
  steps in "For the operator" are still not run.

## 2026-10-16

Host: macOS 26 (Darwin 25.5.0), arm64. Runner: qa-engineer-1. One run of
`scripts/live/harnesses.sh` with no flag, after every CLI was updated. 13 PASS,
1 FAIL, 5 SKIP. Report: `ai_docs/reports/always-latest-clis/cli-check.md`.

Versions: agy 1.4.0, opencode 1.19.3, pi 1.3.0 on node v24.21.0, prime-agent
0.10.0 (a Rust build; see the Prime row).

| Step | Claim it proves | Tool version | Result | Evidence |
|---|---|---|---|---|
| agy-version | U-28: `agy` is installed | agy 1.4.0 | PASS | `agy --version` prints 1.4.0 |
| agy-flags | U-28: the 7 adapter flags exist | 1.4.0 | PASS | all 7 in `agy --help` |
| agy-model | U-28: `gemini-3.8-flash` with `--effort` | 1.4.0 | PASS | `agy models` lists `gemini-3.8-flash-low/medium/high` |
| agy-no-api-key | U-28 step 5: no Gemini key is needed | 1.4.0 | PASS | `settings.json` has no `modelProvider` |
| agy-trust-store | U-62: `trustedWorkspaces` is a list of path strings | 1.4.0 | PASS | list of path strings |
| agy-turn | U-28: the teammate argv runs; `agy` mints the id | 1.4.0 | PASS | SUCCESS, `conversation_id` minted by `agy` |
| agy-resume | U-28 step 11: `--conversation <id>` resumes | 1.4.0 | PASS | same id, recalls the first turn |
| agy-mode-plan | U-28: `--mode plan` | 1.4.0 | PASS | accepted |
| agy-resume-prompt | W13 C6: `--conversation <id> --prompt-interactive <p>` delivers `<p>` | 1.4.0 | SKIP | the TUI asks to trust the scratch repo; needs `AGY_TRUST_SCRATCH=1` |
| agy-print-no-trust | U-62: print mode records no trust | 1.4.0 | PASS | the scratch repo is not in `trustedWorkspaces` |
| agy-bare-needs-effort | W13 C6: a bare id needs `--effort` (`roster/validation.rs`) | 1.4.0 | FAIL | `agy` 1.4.0 accepted `--model gemini-3.8-flash` without `--effort`; the roster rule is stricter than `agy` |
| agy-effort-refusal | `agy` refuses `--effort` for an id it does not list | 1.4.0 | PASS | refused for `gemini-3-1-pro` |
| agy-no-usage-file | U-60, U-61: no usage or quota file | 1.4.0 | PASS | no such file |
| agy-cache-format | U-28 step 10: `last_conversations.json` format | 1.4.0 | SKIP | needs `AGY_READ_CACHE=1` |
| agy-fleet-spawn | U-28 steps 8 to 11 in a fleet | - | SKIP | needs a fleet and a trusted directory |
| opencode-resume-prompt | U-64: `--prompt` beside `--session` is ignored | opencode 1.19.3 | SKIP | the control failed: a fresh TUI `--prompt` did not arrive. In the same run `opencode run` ended at its 120 s limit with no answer, so the free tier may have been slow |
| prime-status | U-63: Prime reports no daemon start time | prime-agent 0.10.0 | SKIP | the script found no `daemon-ps.js`: the installed Prime is a Rust build with no `dist/cli` |
| pi-version | U-30 Q1: pi runs on the installed Node | pi 1.3.0, node v24.21.0 | PASS | `pi --version` prints `1.3.0` |

The step `opencode-version` also passed (1.19.3).

## 2026-10-17: Prime adapter, Rust build (prime-agent 0.10.0)

Host: macOS 26 (Darwin 25.5.0), arm64. Runner: opus-2. No model turn and no
prompt: the TUI showed Prime's first-run "Share agent traces" question, which
was not answered, and Ctrl+C quit it.

Method: a scratch herdr workspace (`w54`, cwd
`.worktrees/_scratch/prime-adapter/live`) ran the ignored test
`live_prime_tui_start_and_finish` (`crates/horch-core/tests/context_prime.rs`)
with `HORCH_STATE_DIR=/tmp/hpa-live`. It runs the Prime adapter's `prepare`,
the command horch builds without `--` and the prompt, and `finish`.

| Check | Result | Evidence |
|---|---|---|
| argv | PASS | `prime-agent --model anthropic/claude-opus-5-5 --thinking medium --no-skills --no-prompt-templates --no-themes --daemon-socket /tmp/hpa-live/prime/acc05e7c0e30/prime-agent-501/d.sock --session-dir /tmp/hpa-live/prime/acc05e7c0e30/sessions`; Prime started (no "Unknown option") |
| built-in reporter | PASS | `herdr agent list`: pane `w54:p1` `"agent":"prime-agent"`, `"agent_status":"idle"`. So `STATUS_AGENT` is `prime-agent` (source `herdr:pi`, from the binary) |
| scoped status | PASS | before finish, `TMPDIR=<launch>/ prime-agent status --json` named `<launch>/prime-agent-501/d.sock`, pid 32121, `status` `current` |
| finish stops the daemon | PASS | after finish, the same command printed `[]`; `ps` showed no `prime-agent` process; the scan dir and `agent/` were removed; `sessions/` kept the `.jsonl` |
| release | PASS | after finish, `herdr pane get w54:p1` showed no agent, `agent_status` `unknown` |

## 2026-10-20: the google pool probe (agy 1.4.3)

Host: macOS 26 (Darwin 25.5.0), arm64. `agy` 1.4.3 at `~/.local/bin/agy`.

U-61 said `agy` has no usage signal. 1.4.3 has one: the `/usage` slash command
in print mode. `routing/quota_probe.rs:probe_google` runs it.

| Check | Result | Evidence |
|---|---|---|
| `/usage` runs no model turn | PASS | `agy -p "/usage" --output-format json --print-timeout 60s`: `status` `SUCCESS`, `num_turns` 0, every `usage` count 0, `conversation_id` empty |
| reply shape | PASS | `command.data.groups[]`: "Gemini Models" (`gemini-weekly`, `gemini-5h`), "Claude and GPT models" (`3p-weekly`, `3p-5h`), "Gemini 4 Argon" (`gemini-argon-wtu-weekly`). Each bucket has `window`, `remaining_fraction` 0-1, `reset_time` UTC. The field names are not documented |
| time | PASS | 3.3 s and 3.7 s wall time: inside the 10 s probe timeout |
| probe | PASS | `HORCH_STATE_DIR=<scratch> horch quota --refresh` (dev build) wrote 9 google windows with `source` `usage`; `horch quota` shows `google ... argon 0% ok` |
| routing | PASS | `horch route spec-writer`: `SPAWN` on the live reading. With the Argon window set to 100% (`HORCH_QUOTA_FILE`): `SUBSTITUTED: spec-writer runs on opus`, while `horch route antigravity` (Flash) still spawns |

Not covered: the reply when the login has expired or the quota is spent.
`parse_agy_usage` treats any `status` other than `SUCCESS` as a probe error,
so the pool is then `unknown` and the Argon teammates run on their fallback.
