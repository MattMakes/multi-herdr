# Live checks

A live check proves a claim against a real tool on the operator's Mac: a
skill line, a doc sentence or a code path that a hermetic test cannot
reach (Unreal Engine, Blender, Xcode, a harness CLI, a real herdr session).
The gate (`scripts/phase-gate.sh`) never runs a live check. The gate stays
hermetic; a live check is re-run by hand when its tool changes.

Each family has 2 files:

- `scripts/live/<family>.sh`: the check. Re-runnable bash.
- `docs/live-checks/<family>.md`: the tracked result. The first lines say
  what the family proves, how to run it and what it needs.

## Run a live check

```bash
scripts/live/<family>.sh
```

Then append the dated result table to `docs/live-checks/<family>.md` and
commit that file. Keep the old tables.

A script:

- prints 1 line per step: `PASS <step>`, `FAIL <step>: <reason>` or
  `SKIP <step>: <reason>`. A missing tool is `SKIP`, not `FAIL`.
- exits non-zero if any step fails.
- writes only under `.worktrees/_scratch/live-<family>/` (git-ignored,
  inside the trusted folder; never `/tmp`), and removes large artifacts at
  the end.
- never reads, prints or sets `ANTHROPIC_API_KEY`, and starts every
  `claude` with `env -u ANTHROPIC_API_KEY`.

Warning: a step that starts a paid model session (a fleet worker, a
`claude -p`, a Codex run) costs tokens. Such a step has a 1-line task, and
its result row says so. No live check starts a new `horch fleet`, because
that starts an orchestrator.

## Families

| family | script | result | proves | needs |
|---|---|---|---|---|
| unreal | `scripts/live/unreal.sh` | [unreal.md](unreal.md) | the `ue-build-verify` and `ue-editor-scripting` commands on the installed UE 5.8: a blank C++ project builds, 1 automation test and 1 Python commandlet run | Unreal Engine 5.8, Xcode; a full build can take up to 60 minutes |
| blender | `scripts/live/blender.sh` | [blender.md](blender.md) | the Blender skill helpers, the pinned Blender MCP server and 1 small asset end to end on the installed Blender | `/Applications/Blender.app`; the asset step uses a blender-artist worker (paid) |
| apple | `scripts/live/apple.sh` | [apple.md](apple.md) | the MobileBuildMCP tool and workflow names that the Swift skills use; the Xcode agent-skills export on Xcode 27 | Xcode, `node`/`npx`, network for the first download; no model session |
| harnesses | `scripts/live/harnesses.sh` | [harnesses.md](harnesses.md) | the Antigravity, OpenCode, Prime and pi harness CLIs: launch, resume, trust and usage claims | each harness CLI and its login; a missing CLI is `SKIP` |
| telemetry | `scripts/live/telemetry.sh` | [telemetry.md](telemetry.md) | the telemetry spec's local acceptance steps (`docs/specs/telemetry.md` §17, L1 to L10) against real sessions | a herdr server; 2 steps spawn 1 worker each (paid); 2 steps need the operator |
| dataset | `scripts/live/dataset.sh` | [dataset.md](dataset.md) | the candidate idle rule (Spec B §4.11.5) with a real Codex candidate: 1 nudge, then `idle_without_done` | a herdr server, `codex`; 1 small round (paid) |
| linux | `scripts/live/linux.sh` | [linux.md](linux.md) | `cargo test --workspace` on aarch64 Linux (the `procid` Linux paths) and Godot headless with `XDG_*` paths | `colima` and Docker; `just test-linux` runs the test step |
| godot-csharp | `scripts/godot/live_csharp.sh` | [godot-csharp.md](godot-csharp.md) | a C# scene and a C# signal run headless on Godot .NET 4.7 (U-24) | Godot .NET 4.7 (`GODOT_MONO_PATH`, else `/Applications/Godot_mono.app`) and `dotnet`; no model session |
| godot-seats | none: 2 worker sessions | [godot-seats.md](godot-seats.md) | the `godot-gameplay-programmer` and `godot-qa-engineer` seats each do 1 real task on Godot 4.7.2 (Godot wave step 5): build Coin Dash headless, then test it with GUT | Godot 4.7.2; 2 worker sessions (paid); GUT from its release tarball (network) |
| godot-codex | `scripts/live/godot-codex.sh` | [godot-codex.md](godot-codex.md) | headless Godot 4.7.2 import, parse check, run and `--doctool` inside a Codex `workspace-write` sandbox, plain and with workspace-local `HOME`/`XDG_*` and the TLS bundle override | Godot 4.7.2; run it in a `codex-sol` pane to prove the sandbox (outside Codex it only proves the script); no model session beyond that pane |
| context | `scripts/live/context.sh` | [context.md](context.md) | the fleet context policy (`docs/specs/context-policy.md`: CTX-05, 06, 13, 15, 17, 18, 20, 26). Part A runs on `$HORCH_BIN` before the install. Part B (`LIVE_CONTEXT_PANES=1`, or `LIVE_CONTEXT_PART=b` for B only) checks the ledger after Claude and Codex pane steps. Part C (`LIVE_CONTEXT_PART=c`; `c5` for the pane steps) checks the OpenCode, pi and Prime readers, the canary, the Prime agent dir and the slice-2 worker panes | `jq`, `perl`, `python3`, `claude` and `codex` signed in (part A); part B needs a herdr server, the installed `horch` and a fleet; part A has paid steps A3, A5, A6, A8 (1 short session each); part C needs `opencode`, `pi`, `prime-agent` and `sqlite3`, and its pane steps need a herdr server |

A family whose result file does not exist yet has not run on this branch.

## Result file format

The header, then 1 dated table for each run, newest last:

```markdown
## 2026-10-06

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| <step> | <claim>, in `<file>` | <tool> <version> | PASS | <observed fact> |
```

- step: the script's step name, as in its `PASS`/`FAIL`/`SKIP` line.
- claim it proves: the skill, doc or code path, with the file.
- tool version: the exact version the step ran against.
- result: `PASS`, `FAIL` or `SKIP`.
- evidence: a short observed fact (a count, an exit code, a log line). Say
  here when the step started a paid model session.

A step that disproves a claim gets the claim fixed in the same unit. A
step that proves a claim marked "unverified" removes the marker and cites
the result file.

## Other checks outside the gate

The `horch smoke` commands and `just verify-perf` also run outside the
gate. [testing-and-gates.md](../testing-and-gates.md) lists them.
