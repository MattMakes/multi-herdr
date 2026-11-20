# Live check: hc and Herdr app backend probe

Probed on 2026-11-19 against `hc 0.1.21` and `herdr-app 0.1.16` on macOS (arm64).
Isolated core questions ran against `HC_SESSION=horch-probe` with scratch `XDG_STATE_HOME` / `XDG_CONFIG_HOME` (`/tmp/hcp.Mujzch`, short enough for macOS's 103-byte Unix socket path limit), pid `65974`, leaving the operator's core (`pid 23875`) untouched. Window questions used a single throwaway space (`horch-probe`, `--focus false`) closed at the end with `herdr-app do space.close --id sp5d56ac9034d7a7c45cc0adfafbb35d7e --confirmed true`.

Operator core before probe:

```text
$ hc server status
core ml-d7vmf4p4ql (fc0e3b58) running, pid 23875, up 21188s
```

Isolated probe core:

```text
$ env -u HC_TERMINAL_ID -u HC_TERMINAL_SOCKET -u HC_SCOPE HC_SESSION=horch-probe XDG_STATE_HOME=/tmp/hcp.Mujzch/s XDG_CONFIG_HOME=/tmp/hcp.Mujzch/c hc server status
core ml-d7vmf4p4ql (f934e5fc) running, pid 65974, up 0s
```

## Q1: Message wrapper on `hc send`

Commands:

```bash
# Start Claude in probe terminal A (t13) with horch's launch flags:
env -u ANTHROPIC_API_KEY -u HC_TERMINAL_ID -u HC_TERMINAL_SOCKET -u HC_SCOPE \
  HC_SESSION=horch-probe XDG_STATE_HOME=/tmp/hcp.Mujzch/s XDG_CONFIG_HOME=/tmp/hcp.Mujzch/c \
  hc run --bg --json --keep --no-parent --cwd /Users/MASCOTT/projects/multi-herdr -- \
  env -u ANTHROPIC_API_KEY claude --mcp-config '{"mcpServers":{}}' --strict-mcp-config \
  --disallowedTools 'Agent,RemoteTrigger,Bash(herdr pane zoom),Bash(herdr pane zoom *)' \
  --model sonnet --effort low --permission-mode auto --plugin-dir "$SCR/bundle" \
  --settings '{"attribution":{"commit":"","pr":"","sessionUrl":false},"disableBundledSkills":true,"disableWorkflows":true,"includeCoAuthoredBy":false,"remoteControlAtStartup":false,"syncClaudeAiPlugins":false,"syncClaudeAiSkills":false}' \
  --session-id fcca8b16-83e6-4e0c-9122-59538114bc25

# (a) From a shell outside any hc terminal:
env -u HC_TERMINAL_ID -u HC_TERMINAL_SOCKET -u HC_SCOPE \
  HC_SESSION=horch-probe XDG_STATE_HOME=/tmp/hcp.Mujzch/s XDG_CONFIG_HOME=/tmp/hcp.Mujzch/c \
  hc send --json t9 'Reply with the single word ALPHA.'

# (b) From inside probe terminal B (t14, named "lead") with its environment as-is:
hc send --json t13 'Reply with the single word BETA.'

# (c) From inside probe terminal B (t14) with HC_TERMINAL_ID and HC_SCOPE removed:
env -u HC_TERMINAL_ID -u HC_SCOPE hc send --json t13 'Reply with the single word GAMMA.'
```

Raw output:

```text
# (a) outside any hc terminal -> hc read t9 --lines 45:
❯ Reply with the single word ALPHA.

⏺ ALPHA

✻ Worked for 6s · done 6:34 PM

# (b) inside probe terminal B (t14) environment and send output:
HC_SCOPE=11219666491119-443b832b456feff38a1a2a5bdd9d44fe
HC_SESSION=horch-probe
HC_TERMINAL_ID=t14
HC_TERMINAL_SOCKET=/tmp/hcp.Mujzch/s/herdr-core/horch-probe/horch-probe.sock
{"started":true,"status":"idle","turn":0}
EXIT=0

# (b) + (c) screen on t13 -> hc read t13 --lines 55:
 ▐▛███▛█   Claude Code v2.1.329
▝▜██████▀  Sonnet 5.5 with low effort · Claude Enterprise
 ▝▝   ▝▝   ~/projects/multi-herdr

❯ Reply with the single word BETA.

⏺ BETA

✻ Crunched for 2s · done 10:19 PM

❯ Reply with the single word GAMMA.

⏺ GAMMA

✻ Sautéed for 2s · done 10:19 PM
```

Observed: None of (a) outside any hc terminal, (b) inside probe terminal B with `HC_TERMINAL_ID=t14` and `HC_SCOPE` set, or (c) inside probe terminal B with `env -u HC_TERMINAL_ID -u HC_SCOPE` wrapped the message in `<hc ...>`.
Rule: Rule A — no `<hc ...>` wrapper is added on any of the three paths, so no environment scrubbing is required for wrapper suppression.

## Q2: `hc send` timing and abandoned sends

Commands:

```bash
# 1. Idle send timing:
hc send --json t9 'Reply with the single word IDLETIMED.'

# 2. Working send timing (while Claude runs `sleep 8` and status.state == "working"):
hc send --json t9 'Reply with the single word WORKTIMED.'

# 3. Unfinished draft in attached PTY + hc send killed after 2.0 s + 70 s wait:
# Attach PTY (`hc attach t13`), type `draft` without Enter, start:
hc send --json t13 'Reply with ABANDONED_PTY_DRAFT.'
# Kill the `hc send` client PID at 2.0 s while still waiting on draft, sleep 70 s, then:
hc read t13 --lines 55
```

Raw output:

```text
idle_send:
  cmd: hc send --json t9 Reply with the single word IDLETIMED.
  code: 0
  stdout: {"started":true,"status":"done","turn":3}
  elapsed_s: 0.27

working_send (saw_working=True):
  cmd: hc send --json t9 Reply with the single word WORKTIMED.
  code: 0
  stdout: {"started":true,"status":"working","turn":3}
  elapsed_s: 0.376

abandoned_send (with attached PTY holding unfinished draft `❯ draft`):
  poll_at_2s_before_kill: None (still running at 2.0 s when killed with SIGKILL)
  after 70.0 s wait -> hc read t13 --lines 55:
    ❯ Reply with the single word BETA.
    ⏺ BETA
    ❯ Reply with the single word GAMMA.
    ⏺ GAMMA
    ────────────────────────────────────────────────────────
    ❯ draft
    ────────────────────────────────────────────────────────
  get_after_70s: {"address":"t13","state":"live","status":{"app":"claude-code","state":"done"},"reported":true,"turn":2,"status_ms":1795151996597}
  ABANDONED_PTY_DRAFT in screen: False
```

Observed: `hc send` took `0.270 s` when Claude was idle and `0.376 s` when Claude was working; an `hc send` blocked on an unfinished human draft and killed after 2 s never arrived during the following 70 s.
Rule: Rule A — the killed send never arrives, so `HcApp::agent_prompt` honors the caller's timeout and deadline just as `Herdr` does.

## Q3: Agent status reporting by harness

Commands:

```bash
# claude (Claude Code 2.1.329):
hc run --bg --json --keep --no-parent --cwd /Users/MASCOTT/projects/multi-herdr -- \
  env -u ANTHROPIC_API_KEY claude --mcp-config '{"mcpServers":{}}' --strict-mcp-config \
  --disallowedTools 'Agent,RemoteTrigger,Bash(herdr pane zoom),Bash(herdr pane zoom *)' \
  --model sonnet --effort low --permission-mode auto --plugin-dir "$SCR/bundle" \
  --settings '{"attribution":{"commit":"","pr":"","sessionUrl":false},"disableBundledSkills":true,"disableWorkflows":true,"includeCoAuthoredBy":false,"remoteControlAtStartup":false,"syncClaudeAiPlugins":false,"syncClaudeAiSkills":false}' \
  --session-id 862f234d-893e-4d9a-8d1c-7eab7b3d0dee

# codex (--no-daemon):
hc run --bg --json --keep --no-parent --cwd /Users/MASCOTT/projects/multi-herdr -- \
  codex --no-daemon -c 'model="gpt-5.6-sol"' -c check_for_update_on_startup=false \
  -c 'tui.resume_cwd="current"' --dangerously-bypass-hook-trust -s read-only -a never \
  --search -c 'model_reasoning_effort="low"' -c tui.auto_recap=false -c 'notify=[]' \
  -c features.multi_agent=false

# opencode (OpenCode 1.19.3):
hc run --bg --json --keep --no-parent --cwd "$SCR/repo" -- \
  env 'OPENCODE_CONFIG_CONTENT={"agent":{"title":{"disable":true}},"skills":{"paths":[]}}' \
  opencode --model opencode/nemotron-3.5-lightning-free --auto --pure

# pi:
hc run --bg --json --keep --no-parent --cwd /Users/MASCOTT/projects/multi-herdr -- \
  pi --model ollama/qwen3.6:35b --thinking low --no-extensions --no-skills \
  --no-prompt-templates --no-themes --session-id <uuid> --skill "$SCR/bundle/skills"

# prime:
which prime-agent

# antigravity (agy):
hc run --bg --json --keep --no-parent --cwd /Users/MASCOTT/projects/multi-herdr -- \
  env -u ANTHROPIC_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GOOGLE_GEMINI_BASE_URL \
  agy --model gemini-3.8-flash --effort low --sandbox --dangerously-skip-permissions
```

Raw output:

```json
{
  "claude": {
    "at_prompt":     {"address": "t9",  "state": "live", "status": {"app": "claude-code", "state": "idle"},    "reported": true,  "turn": 0, "status_ms": 1795138450594},
    "while_working": {"address": "t9",  "state": "live", "status": {"app": "claude-code", "state": "working"}, "reported": true,  "turn": 0, "status_ms": 1795138454293},
    "after_stop":    {"address": "t9",  "state": "live", "status": {"app": "claude-code", "state": "done"},    "reported": true,  "turn": 1, "status_ms": 1795138460799}
  },
  "codex": {
    "at_prompt":     {"address": "t11", "state": "live", "status": {"app": "codex", "state": "idle"},          "reported": true,  "turn": 0, "status_ms": 1795145554349},
    "while_working": {"address": "t11", "state": "live", "status": {"app": "codex", "state": "working"},       "reported": true,  "turn": 0, "status_ms": 1795145565496},
    "after_stop":    {"address": "t11", "state": "live", "status": {"app": "codex", "state": "done"},          "reported": true,  "turn": 1, "status_ms": 1795146599054}
  },
  "opencode": {
    "at_prompt":     {"address": "t6",  "state": "live", "status": null,                                       "reported": false, "turn": 0, "status_ms": null},
    "while_working": null,
    "after_stop":    {"address": "t6",  "state": "live", "status": null,                                       "reported": false, "turn": 0, "status_ms": null}
  },
  "pi": {
    "at_prompt":     {"address": "t12", "state": "live", "status": {"app": "pi", "state": "idle"},             "reported": true,  "turn": 0, "status_ms": 1795146602244},
    "while_working": {"address": "t12", "state": "live", "status": {"app": "pi", "state": "working"},          "reported": true,  "turn": 0, "status_ms": 1795146605138},
    "after_stop":    {"address": "t12", "state": "live", "status": {"app": "pi", "msg": "404: {\"message\":\"model 'qwen3.6:35b' not found\",\"type\":\"not_found_error\",\"param\":null,\"code\":null}", "state": "error"}, "reported": true, "turn": 1, "status_ms": 1795146605212}
  },
  "prime": {
    "installed": false,
    "which": "prime-agent not found"
  },
  "antigravity": {
    "at_prompt":     {"address": "t8",  "state": "live", "status": {"app": "antigravity", "state": "idle"},    "reported": true,  "turn": 0, "status_ms": 1795137943549},
    "while_working": {"address": "t8",  "state": "live", "status": {"app": "antigravity", "state": "working"}, "reported": true,  "turn": 0, "status_ms": 1795137945599},
    "after_stop":    {"address": "t8",  "state": "live", "status": {"app": "antigravity", "msg": "PONG", "state": "done"}, "reported": true, "turn": 1, "status_ms": 1795137947399}
  }
}
```

Observed: `claude`, `codex` (`--no-daemon`), and `antigravity` reported `idle` at their first prompt, went to `working`, and returned to `done` (`turn: 1`); `opencode` answered the prompt on screen while keeping `reported: false` and `status: null`; `pi` reported `idle` then `working` then `error` because no Ollama chat model was pulled locally; `prime-agent` is not installed.
Rule: `HC_STATUS_HARNESSES` in Task 14 is `&[HarnessKind::Claude, HarnessKind::Codex, HarnessKind::Antigravity]`.

## Q4: `herdr-app do pane.split` parent and focus behavior

Commands:

```bash
herdr-app status
herdr-app do space.new --name horch-probe --focus false
herdr-app do pane.split '{"direction":"right","pane":"pbaa04f05bd3f25daf7a028e85cbc6984"}'
hc get --json t4
herdr-app status
```

Raw output:

```json
// status before split:
{
  "zoom": 1.0,
  "font_size": 14.0,
  "ui_font_size": 13.0,
  "space": "sp7b55a774da47cc165a3bec47fde590f4",
  "tab": "tb68879a84bb3bb9f8ebfd8dc820415d2b",
  "focused_pane": "p60739e5cbce0e68c105c28257586205e",
  "layout": "main",
  "agent": "t2",
  "floats": {},
  "overlays": [],
  "errors": []
}

// pane.split result:
{
  "pane": "p29c4101e5044bddfb3262f74de1bd5e4",
  "terminal": "t4"
}

// hc get --json t4:
{"address":"t4","argv":["/bin/zsh","-l"],"cols":200,"created_ms":1795137174240,"cwd":"/Users/MASCOTT/projects/multi-herdr","die_with_parent":false,"exit_code":null,"finished_ms":null,"identity":"ghostty","keep":true,"labels":{"herdr.seen":"0","herdr.space":"sp5d56ac9034d7a7c45cc0adfafbb35d7e","herdr.tab":"tbfdeed5b45cc79ba300cbf1c07d7457b6"},"name":null,"notify":true,"parent":null,"pid":66085,"reason":null,"reported":false,"rm":false,"rows":120,"screen":"exact","signal":null,"state":"live","status":null,"status_ms":null,"template":null,"title":"","turn":0}

// status after split:
{
  "zoom": 1.0,
  "font_size": 14.0,
  "ui_font_size": 13.0,
  "space": "sp7b55a774da47cc165a3bec47fde590f4",
  "tab": "tb68879a84bb3bb9f8ebfd8dc820415d2b",
  "focused_pane": "p60739e5cbce0e68c105c28257586205e",
  "layout": "main",
  "agent": "t2",
  "floats": {},
  "overlays": [],
  "errors": []
}
```

Observed: `hc get --json t4` has `"parent": null` and `"keep": true`, and `herdr-app status` shows `space`, `tab`, `focused_pane`, and `agent` unchanged after the split.
Rule: Rule A — `pane_split` uses `herdr-app do pane.split` as its primary path and needs no post-split `pane.focus` restore.

## Q5: App adoption of a labelled `hc run` terminal

Commands:

```bash
env -u HC_TERMINAL_ID -u HC_SCOPE hc run --bg --json --keep --no-parent \
  --label herdr.space=sp5d56ac9034d7a7c45cc0adfafbb35d7e \
  --label herdr.tab=tbfdeed5b45cc79ba300cbf1c07d7457b6
herdr-app ls
```

Raw output:

```json
// hc run:
{"address":"t5","agent":"zsh","cwd":"/Users/MASCOTT/projects/multi-herdr","state":"live","turn":0}

// matching space in herdr-app ls:
{
  "id": "sp5d56ac9034d7a7c45cc0adfafbb35d7e",
  "name": "horch-probe",
  "folder": "/Users/MASCOTT/projects/multi-herdr",
  "machine": null,
  "active": false,
  "tabs": [
    {
      "id": "tbfdeed5b45cc79ba300cbf1c07d7457b6",
      "name": "1",
      "active": true,
      "settled": false,
      "terminals": [
        {
          "terminal": "t3",
          "pane": "pbaa04f05bd3f25daf7a028e85cbc6984",
          "status": null,
          "app": null,
          "name": null,
          "sub_agents": []
        },
        {
          "terminal": "t4",
          "pane": "p29c4101e5044bddfb3262f74de1bd5e4",
          "status": null,
          "app": null,
          "name": null,
          "sub_agents": []
        },
        {
          "terminal": "t5",
          "pane": "p94808d2ef1dc4948fa3168e78bbd0f8b",
          "status": null,
          "app": null,
          "name": null,
          "sub_agents": []
        }
      ]
    }
  ]
}
```

Observed: `herdr-app ls` lists `t5` inside tab `tbfdeed5b45cc79ba300cbf1c07d7457b6` of space `sp5d56ac9034d7a7c45cc0adfafbb35d7e` with assigned pane id `p94808d2ef1dc4948fa3168e78bbd0f8b`.
Rule: Labelled `hc run --bg --json --keep --no-parent --label herdr.space=<ws> --label herdr.tab=<tab>` is adopted into that space and tab by the Herdr app, so the window-closed split fallback works as designed.

## Q6: `herdr-app` behavior when no window is open

Commands:

```bash
HERDR_APP_HOME=/tmp/hcp.Mujzch/app herdr-app status
HERDR_APP_HOME=/tmp/hcp.Mujzch/app herdr-app do space.new --name x
```

Raw output:

```text
herdr-app: no Herdr window at /tmp/hcp.Mujzch/app/herdr-app.sock: No such file or directory (os error 2)
status_exit=2
herdr-app: no Herdr window at /tmp/hcp.Mujzch/app/herdr-app.sock: No such file or directory (os error 2)
space_new_exit=2
```

Observed: Both `herdr-app status` and `herdr-app do space.new --name x` exit with code `2` and print `herdr-app: no Herdr window at <HERDR_APP_HOME>/herdr-app.sock: No such file or directory (os error 2)` on stderr.
Rule: `HcApp` detects a closed or absent window by non-zero exit status from `herdr-app`, never by matching the error message text, and `fake-herdr-app` emits this stderr line when `window-closed` exists.

## Q7: Space, tab, and terminal ID formats vs `validate_plain`

Commands:

```bash
herdr-app do space.new --name horch-probe --focus false
```

Raw output:

```json
{
  "space": "sp5d56ac9034d7a7c45cc0adfafbb35d7e",
  "tab": "tbfdeed5b45cc79ba300cbf1c07d7457b6",
  "terminal": "t3"
}
```

`crates/horch-core/src/ids.rs:54-62`:

```rust
fn validate_plain(kind: &'static str, value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        Err(IdError::Empty { kind })
    } else if value.chars().any(char::is_control) {
        Err(IdError::ControlChar { kind })
    } else {
        Ok(())
    }
}
```

Observed: `space.new` returns `sp` + 32 lowercase hex digits (`sp5d56ac9034d7a7c45cc0adfafbb35d7e`), `tb` + 32 lowercase hex digits (`tbfdeed5b45cc79ba300cbf1c07d7457b6`), and `t<N>` (`t3`), all of which are non-empty ASCII strings without control characters or `:` and pass `PaneId::new` and `WorkspaceId::new` (`validate_plain`).
Rule: `PaneId` and `WorkspaceId` accept `t<N>` and `sp<32 hex>` as-is without changing `ids.rs`.

## Q8: Immediate `hc send` of a quoted launch line right after `pane.split`

Commands:

```bash
for i in 1 2 3 4 5; do
  SJSON=$(herdr-app do pane.split '{"direction":"down","pane":"pbaa04f05bd3f25daf7a028e85cbc6984"}')
  STERM=$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["terminal"])' "$SJSON")
  LINE="unset ANTHROPIC_API_KEY; '/usr/bin/printf' '%s|%s\n' 'it'\\''s a \"quote\" test' 'run $i' > '$SCR/q8_$i.txt'"
  env -u HC_TERMINAL_ID -u HC_SCOPE hc send --json "$STERM" "$LINE"
done
```

Raw output:

```text
iter 1: split={"pane":"p32c6f47893812f5426ebb819f4433e0e","terminal":"t6"} send={"started":null,"status":null,"turn":0}
iter 2: split={"pane":"p83611dda3cefc36492f2e04ae59b5309","terminal":"t7"} send={"started":null,"status":null,"turn":0}
iter 3: split={"pane":"pf500d6a1ed3606a29102b72648b75a13","terminal":"t8"} send={"started":null,"status":null,"turn":0}
iter 4: split={"pane":"p117e6f09fcb7154789786d196cfdd7c5","terminal":"t9"} send={"started":null,"status":null,"turn":0}
iter 5: split={"pane":"p64bb7b6932f38a5d5068239e92920863","terminal":"t10"} send={"started":null,"status":null,"turn":0}
q8_1.txt: it's a "quote" test|run 1
q8_2.txt: it's a "quote" test|run 2
q8_3.txt: it's a "quote" test|run 3
q8_4.txt: it's a "quote" test|run 4
q8_5.txt: it's a "quote" test|run 5
```

Observed: All 5 immediate post-split `hc send` calls ran the `PaneShell::Posix` command line (with spaces, escaped single quotes, and double quotes) unchanged (5 of 5).
Rule: Rule A — `pane_run` on `HcApp` is `hc send`, and `workspace_create`, `pane_split`, and `tab_new` do not need to pass a launch command as `argv`.

---

## Live check: real Herdr app fleet beside a TUI fleet

Verified on 2026-11-20 against `horch 0.5.1` installed via `just install` from branch `feat/hc-backend`, running in the live Herdr app (`hc 0.1.21`, `herdr-app 0.1.16`, `herdr-core` pid `23875`) alongside the live `herdr` TUI server (`herdr 0.9.4-preview`). Two scratch repositories (`/tmp/hc-live-check/proj-hc` and `/private/tmp/hc-live-check/proj-herdr`) shared an isolated state root (`/tmp/hc-live-check/state`) and task ledger (`/tmp/hc-live-check/ledger.sqlite3`) so the operator's workspaces and ledgers were untouched.

Four real-world issues found during this live check were reproduced with unit/e2e tests, fixed, reinstalled, and re-verified before completing the run:

- `pane_command_for` now passes `HORCH_PROJECT_DIR` so symlinked `--cwd` paths (such as `/tmp/...` vs `/private/tmp/...` on macOS) match the session ledger slug inside the new terminal.
- `recipes::fleet_with` skips the `herdr`-only `resume_agents_on_restore` warning when `ctx.herdr.backend == Backend::Hc`.
- `HcApp::hc_command` removes `HC_TERMINAL_ID` and `HC_SCOPE` from every `hc` child (Q1 Rule B) after Step 4 showed `horch tell` from inside a Claude orchestrator terminal (`t17`) received `<hc from="t17">...</hc>` when those variables remained set.
- `workspace::place::address_key` parses `hc` terminal suffixes in base 36 rather than base 10 after live spawns produced Crockford base-32/36 addresses (`t1c`, `t1j`, `t1k`, `t1m`, `t1n`, `t1p`).

### 1. Install, `horch doctor`, and `herdr-fleet` in an app terminal

Commands:

```bash
just install
horch doctor
cd /tmp/hc-live-check/proj-hc
HORCH_STATE_DIR=/tmp/hc-live-check/state HORCH_LEDGER_DB=/tmp/hc-live-check/ledger.sqlite3 \
  herdr-fleet --no-update --verify minimal
herdr-app shot docs/live-checks/hc-backend-1.png
```

Picture: `docs/live-checks/hc-backend-1.png`

Seen: `horch doctor` printed `hc is installed and reachable.`, `also found: herdr (reachable)`, `delivery: confirmed (typed echo executed).`, and the core-socket sandbox note; `herdr-fleet` opened space `spde560c75f8875952d3ec9df9b89c7a34` (`proj-hc`) with Claude Opus orchestrator `t17`, whose channel relay connected (`orchestrator t17 (channel)`) and acknowledged the delivery probe.

### 2. Two-ticket brief spawns 2 workers in 2 columns right of the orchestrator

Commands:

```bash
env -u HC_TERMINAL_ID -u HC_SCOPE hc send --json t17 \
  'Live check (2-ticket brief): run `horch ledger run new live-check --steps check` ... Queue both tickets (T-1 alpha.txt, T-2 beta.txt) and spawn 2 `sonnet` workers immediately.'
herdr-app ls
herdr-app shot docs/live-checks/hc-backend-2.png
```

Picture: `docs/live-checks/hc-backend-2.png`

Seen: 2 Claude Sonnet workers (`sonnet-1` on `t19` and `sonnet-2` on `t1b`) appeared to the right of orchestrator `t17` in 2 columns; `herdr-app ls` reported 3 terminals (`t17`, `t19`, `t1b`) in tab 1 (`tbc42b460c9bc6239c8f1debaa3253d884`).

### 3. Worker receives its task with no `<hc` wrapper on screen

Commands:

```bash
hc read t19 --lines 50
hc read t19 --lines 50 | grep -c '<hc' || true
```

Seen: `sonnet-1` (`t19`) claimed T-1, created `alpha.txt` in its worktree, committed it, and queued its question to the orchestrator with `0` occurrences of `<hc` on screen.

### 4. `horch tell` arrives once (unwrapped) and `horch done` closes the pane

Commands:

```bash
env -u HC_TERMINAL_ID -u HC_SCOPE hc send --json t17 \
  'Answer sonnet-2 now with: `horch tell sonnet-2 --answers 43188222 "Confirmed BETA_TELL_ONCE: run horch ledger finish --wait and then horch ledger done \"Added beta.txt\" now."`'
hc read t1b --lines 50
hc ls
herdr-app shot docs/live-checks/hc-backend-4.png
```

Picture: `docs/live-checks/hc-backend-4.png`

Seen: After scrubbing `HC_TERMINAL_ID` and `HC_SCOPE` in `HcApp::hc_command` and reinstalling, `[orchestrator re #43188222] Confirmed BETA_TELL_ONCE: ...` arrived once on `t1b` with `0` `<hc` wrapper lines; `sonnet-2`'s `horch ledger done` closed its pane and `t1b` left `hc ls` (and `hc ls --all`).

### 5. Window closed: orchestrator spawns a worker via labelled `hc run`, adopted on reopen

Commands:

```bash
osascript -e 'tell application "Herdr" to quit'
herdr-app status   # exited 2 while herdr-core pid 23875 stayed up
env -u HC_TERMINAL_ID -u HC_SCOPE hc send --json t17 \
  'Queue one tiny task T-3 (gamma.txt, --primary sonnet --check none) and run `horch spawn sonnet` right now.'
hc ls --json --label herdr.space=spde560c75f8875952d3ec9df9b89c7a34
open -a Herdr
herdr-app ls
herdr-app shot docs/live-checks/hc-backend-5.png
```

Picture: `docs/live-checks/hc-backend-5.png`

Seen: With the Herdr window closed (`herdr-app status` exit 2, core pid `23875` still running), `horch spawn sonnet` fell back to labelled `hc run` and started worker `t1c` (`sonnet-3`); reopening the window with `open -a Herdr` adopted `t1c` into space `spde560c75f8875952d3ec9df9b89c7a34` tab 1 (`p406daea8daf0d0601835f22ef7046e25`).

### 6. Parallel `herdr` TUI fleet beside the Herdr app fleet, `doctor --clean`, and `layout`

Commands:

```bash
cd /private/tmp/hc-live-check/proj-herdr
env -u HC_TERMINAL_ID -u HC_SCOPE HORCH_BACKEND=herdr \
  HORCH_STATE_DIR=/tmp/hc-live-check/state HORCH_LEDGER_DB=/tmp/hc-live-check/ledger.sqlite3 \
  herdr-fleet --no-update --verify minimal
env -u HERDR_PANE_ID HC_TERMINAL_ID=t17 HORCH_BACKEND=hc HORCH_STATE_DIR=/tmp/hc-live-check/state HORCH_LEDGER_DB=/tmp/hc-live-check/ledger.sqlite3 horch doctor --clean
env -u HC_TERMINAL_ID -u HC_SCOPE HERDR_PANE_ID=wGF:p1 HORCH_BACKEND=herdr HORCH_STATE_DIR=/tmp/hc-live-check/state HORCH_LEDGER_DB=/tmp/hc-live-check/ledger.sqlite3 horch doctor --clean
ls -d /tmp/hc-live-check/state/herdr-orchestration-*
env -u HERDR_PANE_ID HC_TERMINAL_ID=t17 HORCH_BACKEND=hc HORCH_STATE_DIR=/tmp/hc-live-check/state horch layout
env -u HC_TERMINAL_ID -u HC_SCOPE HERDR_PANE_ID=wGF:p1 HORCH_BACKEND=herdr HORCH_STATE_DIR=/tmp/hc-live-check/state horch layout
herdr-app shot docs/live-checks/hc-backend-6.png
```

Picture: `docs/live-checks/hc-backend-6.png`

Seen: `herdr` TUI workspace `wGF` (`wGF:p1`) ran concurrently beside Herdr app space `spde560c75f8875952d3ec9df9b89c7a34`; `horch doctor --clean` from `t17` and from `wGF:p1` each reported `stale state: removed 0` and kept both state folders intact; `horch layout` printed the sizeless space/tab/terminal tree on `hc` and the `124x42` geometry grid on `herdr`.

### 7. Six live workers overflow to a second tab named `workers 2`

Commands:

```bash
# With sonnet-3 (t1c) already live, queue 5 hold tasks and spawn workers 2..6:
for i in 2 3 4 5 6; do
  env -u HERDR_PANE_ID HC_TERMINAL_ID=t17 HORCH_BACKEND=hc \
    HORCH_STATE_DIR=/tmp/hc-live-check/state HORCH_LEDGER_DB=/tmp/hc-live-check/ledger.sqlite3 \
    HORCH_PROJECT_DIR=/tmp/hc-live-check/proj-hc horch spawn sonnet --force
done
herdr-app ls
env -u HERDR_PANE_ID HC_TERMINAL_ID=t17 HORCH_BACKEND=hc HORCH_STATE_DIR=/tmp/hc-live-check/state horch layout
herdr-app do tab.select '{"id":"tb0abb3e5f769dbfdc64566a952ab82962"}'
herdr-app shot docs/live-checks/hc-backend-7.png
```

Picture: `docs/live-checks/hc-backend-7.png`

Seen: Workers 1–4 (`t1c`, `t1j`, `t1k`, `t1m`) filled tab 1 beside orchestrator `t17`, and workers 5–6 (`t1n`, `t1p`) opened and populated tab 2 named `workers 2` (`tb0abb3e5f769dbfdc64566a952ab82962`).

### 8. No `ANTHROPIC_API_KEY` in any launched agent process

Commands:

```bash
for p in 44597 44940 74781 75527 75646 76220 16844 17888 27757 \
         35706 36200 36408 36870 37194 37911 38232 38954 39259 40355; do
  ps eww "$p" | grep -c ANTHROPIC_API_KEY || true
done
```

Seen: Every supervisor and Claude agent PID launched across steps 1–7 (`t17`, `t19`, `t1b`, `t1c`, `t1j`, `t1k`, `t1m`, `t1n`, `t1p`, and `herdr` orchestrator `wGF:p1` pid `27757`) showed `0` occurrences of `ANTHROPIC_API_KEY` in `ps eww`. After cleanup, `hc ls --all` contained only the operator's `t1` and `t2` terminals and `herdr-core` remained on pid `23875`.
