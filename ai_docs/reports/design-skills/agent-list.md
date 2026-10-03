# D09 agent-list: `horch agent-list`

Branch `ds/agent-list`. Commits: `Harness: Add the agent inventory`, `CLI: Add horch agent-list`.

## What it does

- `horch agent-list [--json] [--no-probe]` prints one row per real harness.
- Each row has: binary path, `--version`, efforts, capabilities (resume, headless, skill exposure), models the roster uses with the teammates and their efforts, pool names, pool state, and `available`.
- `available` is true when the binary is found and the pool state is not `exhausted`.
- The pool state is the worst state of the harness's pools in the cached quota view. The command never refreshes the quota.
- The probe runs `--version` once per found binary, in parallel threads, with the 5 s limit of `harness_version`.

## Files

- `crates/horch-core/src/harness/inventory.rs`: pure `inventory(teammates, facts, view)` and its table tests (`agent_list_inventory_*`).
- `crates/horch-core/src/harness/mod.rs`: `mod inventory;` and the all-kinds list.
- `crates/horch/src/cmd/agentlist.rs`, `cmd/mod.rs`, `main.rs`: the command.
- `crates/horch/tests/agent_list.rs`: 3 CLI tests with fake bins.
- `README.md`: 1 line.

## All-kinds list (for D08)

The list is `HarnessKind::ALL` (a `&'static [HarnessKind]`, `None` included) in `harness/mod.rs`.
D08 must add `Antigravity` to `HarnessKind::ALL`. The inventory skips `None` and needs no other edit.
D08 must also add the harness to `HarnessKind::binary`. The command reads the binary through that method.
The existing test constant `ALL` in `harness/mod.rs` tests is separate. It still has 6 entries.

## Sample output (text, this machine, middle models trimmed)

```
AGENT     STATUS       POOL       BINARY  VERSION
claude    available    unknown    /Users/mascott/.local/bin/claude  2.1.288 (Claude Code)
            efforts: low medium high xhigh max
            can: resume=true headless=true skills=PluginDir
            model -: orchestration-worker (xhigh)
            model fable: orchestration-orchestrator (xhigh), orchestrator (xhigh)
            model opus: architect-reviewer (high), backend-developer (medium), designer (medium), frontend-developer (medium), judge (high), opus (med
codex     available    unknown    /opt/homebrew/bin/codex  codex-cli 0.160.0
            efforts: none low medium high xhigh max
            can: resume=true headless=false skills=CodexHome
            model gpt-5.6-luna: codex-luna (low)
opencode  available    unknown    /Users/mascott/.opencode/bin/opencode  1.18.2
            efforts: none minimal low medium high xhigh max
            can: resume=true headless=false skills=ConfigPaths
            model opencode/big-pickle: opencode-pickle
pi        available    unknown    /opt/homebrew/bin/pi  (no answer)
            efforts: off minimal low medium high xhigh max
            can: resume=true headless=false skills=SkillFlag
            model ollama/qwen3.8: pi (low)
prime     available    unknown    /opt/homebrew/bin/prime-agent  (no answer)
            efforts: off minimal low medium high xhigh max
            can: resume=true headless=false skills=SkillFlag
            model anthropic/claude-opus-5-5: prime (medium)
```

## Sample output (JSON, one row, `--no-probe`)

```json
{
  "agent": "pi",
  "found": true,
  "path": "/opt/homebrew/bin/pi",
  "version": null,
  "efforts": [
    "off",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max"
  ],
  "capabilities": {
    "resume": true,
    "headless": false,
    "skills": "SkillFlag"
  },
  "models": [
    {
      "model": "ollama/qwen3.8",
      "teammates": [
        {
          "name": "pi",
          "effort": "low"
        }
      ]
    }
  ],
  "pools": [
    "local"
  ],
  "pool_state": "unknown",
  "available": true
}
```

## Decisions

- Skill exposure prints as the `SkillExposure` variant name (`PluginDir`). `capabilities.rs` has no string form and is not in scope.
- A harness with no roster teammate uses the pool of an empty model name. For `pool_for`, that is the unknown pool for pi.
- A teammate with no `model` appears under model `null` (`-` in the table).
- "Found" means: an override or path with a separator is an existing file, or a bare name is an executable on `PATH`.

## Gotchas

- Prime Agent prints `--version` on stderr. `harness_version` reads stdout only, so prime shows `(no answer)`. The dataset preflight has the same gap. Fix: read stderr in `quota_probe::run_short`. This file is outside the scope of D09.
- On this machine `pi --version` crashes (Node `ERR_REQUIRE_ESM`). The row shows `(no answer)`. That is correct.
- With no `quota.json` the pool state is `unknown`, and `available` follows the binary alone.

## Follow-ups

- Make `harness_version` fall back to stderr (prime).
- Give `SkillExposure` an `as_str`.
