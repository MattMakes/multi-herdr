# P1 ue-teammates: the 11 Unreal Engine teammates

Branch `ds/ue-teammates`. Worker opus-48. Date 2026-10-03.
Plan: `ai_docs/plans/domain-skills/p1-ue-teammates.md`. Specification:
`ai_docs/reports/unreal-engine-wave.md` "Teammates to add".

## Result

| commit | content |
|---|---|
| `Teammates: Add the 6 wave-1 Unreal Engine teammates` | `ue-tech-lead`, `ue-gameplay-engineer`, `ue-network-engineer`, `ue-technical-artist`, `ue-qa-engineer`, `ue-code-reviewer`; the test lists |
| `Teammates: Add the 5 wave-2 Unreal Engine teammates` | `ue-character-engineer`, `ue-ai-engineer`, `ue-ui-engineer`, `ue-tools-engineer`, `ue-world-engineer`; the test lists; `teammates/README.md` "The Unreal Engine team" |
| this report | |

- `horch teammates --check` (built binary): `roster ok: 52 teammates, 46 offered to the orchestrator`.
- Gate after wave 1: green.
- Gate after wave 2: the first run failed 1 test, `horch-e2e --test fakes fake_prime_creates_session_file`. It passed 3 of 3 alone, and the next full run was green. That test starts the fake `prime-agent` binary and does not read the roster.

## What each file sets

Every file: `base: fleet-worker`, `agent: claude`, `permission_mode: auto`,
`inherit_plugins: false`, `mcp_servers: {}`,
`disabled_skills: [herdr-orchestrator, herdr-worker]`,
`offer_when: ["*.uproject"]`. Name, `brief_description`, phase, model and
effort are the report's tables, unchanged.

| teammate | phase | model / effort | fallbacks | `disallowed_tools` | expected skills (`skills:`) | `available_skills:` |
|---|---|---|---|---|---|---|
| `ue-tech-lead` | plan | opus / high | codex-sol | Agent | ue-project-context, ue-module-build-system, ue-game-features, ue-cpp-foundations | ue-actor-component-architecture, ue-gameplay-framework, ue-testing-debugging |
| `ue-gameplay-engineer` | implementation | opus / medium | codex-sol | Agent | ue-cpp-foundations, ue-actor-component-architecture, ue-gameplay-framework, ue-gameplay-abilities, ue-gameplay-tags-messaging, ue-blueprint-cpp-interop, ue-data-assets-tables, ue-physics-collision, ue-build-verify | ue-networking-replication, ue-game-features, ue-async-threading |
| `ue-network-engineer` | implementation | opus / medium | codex-sol | Agent | ue-networking-replication, ue-gameplay-framework, ue-gameplay-abilities, ue-character-movement, ue-mover, ue-build-verify | ue-actor-component-architecture, ue-cpp-foundations, ue-physics-collision |
| `ue-technical-artist` | implementation | opus / medium | codex-sol | Agent | ue-materials-rendering, ue-niagara-effects, ue-audio-system, ue-sequencer-cinematics, ue-procedural-generation, ue-build-verify, ue-editor-scripting | ue-actor-component-architecture, ue-data-assets-tables, ue-animation-system |
| `ue-qa-engineer` | validation | sonnet / high | codex-terra | Agent | check, debug, tdd, ue-testing-debugging, ue-module-build-system, ue-build-verify | ue-cpp-foundations |
| `ue-code-reviewer` | validation | opus / high | codex-sol | Agent, Edit, Write, NotebookEdit | code-review, ue-cpp-foundations, ue-actor-component-architecture, ue-networking-replication, ue-async-threading | ue-gameplay-framework, ue-gameplay-abilities, ue-blueprint-cpp-interop |
| `ue-character-engineer` | implementation | opus / medium | codex-sol | Agent | ue-character-movement, ue-mover, ue-animation-system, ue-gameplay-cameras, ue-input-system, ue-build-verify | ue-gameplay-framework, ue-actor-component-architecture, ue-gameplay-abilities |
| `ue-ai-engineer` | implementation | opus / medium | codex-sol | Agent | ue-ai-navigation, ue-state-trees, ue-mass-entity, ue-async-threading, ue-build-verify | ue-actor-component-architecture, ue-gameplay-framework, ue-gameplay-tags-messaging |
| `ue-ui-engineer` | implementation | opus / medium | codex-sol | Agent | ue-ui-umg-slate, ue-input-system, ue-blueprint-cpp-interop, ue-build-verify | ue-gameplay-framework, ue-cpp-foundations, ue-gameplay-abilities |
| `ue-tools-engineer` | implementation | opus / medium | codex-sol | Agent | ue-editor-tools, ue-ui-umg-slate, ue-module-build-system, ue-data-assets-tables, ue-blueprint-cpp-interop, ue-build-verify, ue-editor-scripting | ue-cpp-foundations, ue-testing-debugging |
| `ue-world-engineer` | implementation | opus / medium | codex-sol | Agent | ue-world-level-streaming, ue-procedural-generation, ue-physics-collision, ue-serialization-savegames, ue-build-verify | ue-networking-replication, ue-data-assets-tables, ue-actor-component-architecture |

`brief_description` lengths: 101 to 115 characters. Each one is quoted in
YAML, because each has a `:` that YAML otherwise reads as a mapping.

## Decisions

- **`available_skills:` rule.** For each teammate I counted how many of its
  expected skills name each other `ue-*` skill under "Related Skills". I took
  the top names by count, at most 3, and broke ties by fit to the role.
  `ue-qa-engineer` gets 1 and `ue-tools-engineer` gets 2, because their other
  candidates are named only once or twice and do not fit the role. Example:
  `ue-networking-replication` is named by 6 of the gameplay engineer's 8
  skills. The full count table is reproducible with the script idea in
  "How to reproduce" below.
- **Persona rule 2.** `ue-technical-artist` and `ue-tools-engineer` carry
  `ue-editor-scripting`, so their rule 2 says: change an asset only through
  editor Python or a commandlet, never by writing bytes; if it cannot be
  scripted, hand it back. The other 9 keep the hand-back rule.
  `ue-code-reviewer` turns it into a finding ("a diff that changes asset bytes
  by hand is a finding").
- **Rules 4 and 5 for the read-only and planning seats.** `ue-code-reviewer`
  does not build (rule 4) and treats a `DONE:` without target, configuration
  and filter as a finding (rule 5). `ue-tech-lead` owns
  `.agents/ue-project-context.md` (rule 1), and each plan step names the
  target, configuration and filter that prove it (rule 5).
- **No unverified UE 5.8 facts in persona text.** The personas name no build
  flag, no editor binary, no report file name, no Live Coding message text and
  no registry key. Those are the "no" rows in
  `ai_docs/reports/domain-skills/ue-own.md`. Command detail stays in
  `ue-build-verify` and `ue-editor-scripting`, which mark them. Rule 4 says
  "Report Live Coding ... as `BLOCKED:`" and does not quote a message.
- **Fallbacks.** Like the non-UE counterparts: `codex-sol` for the Opus seats,
  `codex-terra` for `ue-qa-engineer`. `--check` validates each fallback route
  with the skills (it prints `<name> via codex-sol` problems when there are
  some). On native Windows, a fallback spawn fails `ensure_skills_supported`
  (`crates/horch-core/src/harness/codex.rs:440`), the same as a Codex UE
  teammate. The README says this.
- **No MCP servers** (`mcp_servers: {}`), as the wave report says. The engine
  headers are the API source.
- **QA keeps `check`, `debug`, `tdd`** from `qa-engineer`, and the reviewer
  keeps `code-review` from `architect-reviewer`, as the wave report's table
  says.

## Test changes (the plan's known places only)

- `crates/horch-core/tests/baseline_oracles.rs` and
  `crates/horch-core/tests/skills_catalog.rs`: the 11 names in
  `SKIP_NEW_TEAMMATES` (sorted). No oracle file is added or changed.
- `crates/horch-core/src/roster/validation.rs`:
  `builtin_phase_defaults_are_portable_and_disabling_conflicts` gets
  `ue-tech-lead` in the plan arm and `ue-qa-engineer`, `ue-code-reviewer` in
  the validation arm. `roster_check_demands_the_subagent_deny_on_every_claude_fleet_pane`
  goes from 24 to 35 covered Claude teammates (34 spawnable plus the
  orchestrator).
- No golden changed. `the_orchestrator_briefing_differs_only_where_sanctioned`
  in `golden_prompts.rs` builds the expected roster block from
  `r.roster_lines()` at run time, so the 11 new lines need no sanctioned
  change.

## Step 3: the orchestrator briefing, with and without `Game.uproject`

Method: the built `horch fleet` against the repo's fake `herdr` (scenario
`exec`, so the pane really runs `horch pane-launch`) and fake `claude`, which
records its argv. The orchestrator briefing is the longest argument of the
`claude` call that has `--session-id`. The script is below. It runs with
`env -i`, so no operator variable (and no API key) reaches the fakes.

```bash
#!/bin/bash
# Usage: ue-demo.sh <worktree> <label> <with-uproject:yes|no>
set -u
WT=$1; LABEL=$2; UP=$3
T=$(mktemp -d /tmp/ue-demo-$LABEL.XXXX)
mkdir -p "$T/project" "$T/home" "$T/state" "$T/bin" "$T/tmp"
[ "$UP" = yes ] && touch "$T/project/Game.uproject"
for n in claude codex herdr opencode pi ollama; do ln -s "$WT/target/debug/fake-$n" "$T/bin/$n"; done
ln -s "$WT/target/debug/fake-prime" "$T/bin/prime-agent"
ln -s "$WT/target/debug/horch" "$T/bin/horch"
ln -s "$(command -v sqlite3)" "$T/bin/sqlite3"
ln -s "$(command -v git)" "$T/bin/git"
env -i PATH="$T/bin:/usr/bin:/bin" HOME="$T/home" TMPDIR="$T/tmp" \
  HORCH_STATE_DIR="$T/state" HORCH_PROJECT_DIR="$T/project" HORCH_FAKE_LOG="$T/log" \
  HORCH_FAKE_SCENARIO=exec CODEX_HOME="$T/home/.codex" HORCH_TEAMMATES_DIR="$WT/teammates" \
  HORCH_CLAUDE_BIN="$T/bin/claude" HORCH_CODEX_BIN="$T/bin/codex" HORCH_HERDR_BIN="$T/bin/herdr" \
  HORCH_OPENCODE_BIN="$T/bin/opencode" HORCH_PI_BIN="$T/bin/pi" HORCH_PRIME_BIN="$T/bin/prime-agent" \
  HORCH_OLLAMA_BIN="$T/bin/ollama" HORCH_SQLITE3_BIN="$T/bin/sqlite3" \
  "$WT/target/debug/horch" fleet --cwd "$T/project" >"$T/fleet.out" 2>&1
echo "$T"
```

Run: `ue-demo.sh $WT ue yes` and `ue-demo.sh $WT plain no`, then read the
argv from `$T/log` (one JSON object per line, `fake: "claude"`). Afterwards
stop the 2 telemetry panes: `pkill -f "state-dir /tmp/ue-demo-"`.

With `Game.uproject` (`/tmp/ue-demo-ue.Bepb`):

```
orchestrator briefing bytes: 16988
roster lines that start with ue-: 11
  ue-ai-engineer          Unreal Engine AI: AIController, behavior trees, EQS, navmesh, State Tree, Smart Objects, Mass crowds.
  ue-character-engineer   Unreal Engine characters: CharacterMovement or Mover modes, AnimInstance, montages, cameras, Enhanced Input.
  ue-code-reviewer        Unreal Engine C++ review: GC and UPROPERTY, lifecycle, net authority, thread safety, 5.8 deprecations. Never edits.
  ue-gameplay-engineer    Unreal Engine gameplay C++: actors, components, GameMode/Pawn/Controller, GAS, tags, data assets, traces.
  ue-network-engineer     Unreal Engine multiplayer: replication, RPCs, GAS prediction, CMC/Mover net movement, dedicated servers.
  ue-qa-engineer          Unreal Engine QA. Builds headless, writes Automation/CQTest tests, reproduces crashes, profiles with Insights.
  ue-tech-lead            Unreal Engine lead. Scans the project into .agents/ue-project-context.md; plans modules, plugins and features.
  ue-technical-artist     Unreal Engine rendering, VFX and audio C++: materials, render targets, Niagara, MetaSounds, Sequencer.
  ue-tools-engineer       Unreal Editor tooling C++: detail customizations, editor utility widgets, menus, asset definitions, validators.
  ue-ui-engineer          Unreal Engine UI: UMG, Slate, Common UI screen stacks, MVVM view models, gamepad focus and input routing.
  ue-world-engineer       Unreal Engine worlds: World Partition, data layers, level streaming, PCG, collision and physics, save games.
```

Without it (`/tmp/ue-demo-plain.2sx6`, empty project):

```
orchestrator briefing bytes: 15515
roster lines that start with ue-: 0
```

The 11 lines cost the orchestrator 1,473 bytes on an Unreal project and 0
bytes elsewhere. The wave report estimated about 1.2 KB.

## Step 4: briefing bytes per teammate

"Briefing" as the wave report defines it: the description bytes of the
expected skills, which `Bundle::briefing` prints in full. Measured from each
`SKILL.md` `description:` (quotes removed). Column 3 uses the report's skill
list; column 4 adds `ue-build-verify` (191 B) and `ue-editor-scripting`
(213 B) where attached; column 5 adds the generic skills (`code-review`,
`check`, `debug`, `tdd`). Column 6 is the `available_skills:` names, which
the briefing prints without descriptions.

| teammate | report estimate | report's skills | + house skills | all expected skills | available names |
|---|---|---|---|---|---|
| `ue-tech-lead` | 2.6 KB | 2,581 B | 2,581 B | 2,581 B | 72 B (3) |
| `ue-gameplay-engineer` | 5.4 KB | 5,398 B | 5,589 B | 5,589 B | 59 B (3) |
| `ue-network-engineer` | 3.3 KB | 3,258 B | 3,449 B | 3,449 B | 69 B (3) |
| `ue-technical-artist` | 3.3 KB | 3,316 B | 3,720 B | 3,720 B | 71 B (3) |
| `ue-qa-engineer` | 1.3 KB | 1,331 B | 1,522 B | 1,814 B | 18 B (1) |
| `ue-code-reviewer` | 2.6 KB | 2,580 B | 2,580 B | 2,690 B | 66 B (3) |
| `ue-character-engineer` | 3.3 KB | 3,291 B | 3,482 B | 3,482 B | 73 B (3) |
| `ue-ai-engineer` | 2.6 KB | 2,655 B | 2,846 B | 2,846 B | 78 B (3) |
| `ue-ui-engineer` | 2.0 KB | 1,965 B | 2,156 B | 2,156 B | 60 B (3) |
| `ue-tools-engineer` | 3.3 KB | 3,305 B | 3,709 B | 3,709 B | 38 B (2) |
| `ue-world-engineer` | 2.7 KB | 2,686 B | 2,877 B | 2,877 B | 77 B (3) |

Each measured value is within 0.06 KB of the report's estimate (1 KB = 1,000 B). The 2 house skills add 191 to
404 B per teammate. The phase catalog paragraph and the harness's own skill
list are not in these numbers.

## How to reproduce the `available_skills` counts

For each teammate, read the `## Related Skills` section of each expected
skill's `SKILL.md`, collect the `ue-*` names, drop the teammate's own expected
skills, and count. Note: `ue-niagara-effects` "Related Skills" has the token
`ue-driven` (prose, not a skill); ignore it.

## Gotchas and follow-ups

- `brief_description` values with `:` must be quoted. Unquoted, the loader
  fails with `mapping values are not allowed in this context`.
- In zsh, `echo =====` fails (`=` expansion). Quote it.
- The `ue-own` "no" rows (Mac/Linux editor binary, `index.json`, the Live
  Coding message, registry key) still need an engine check. The personas do
  not depend on them.
- Not done, out of scope: a sample-project trial (Lyra or a blank 5.8 C++
  template) with one `ue-gameplay-engineer` task and one `ue-qa-engineer`
  task, as the wave report's "Suggested order" step 3 says. No engine is
  installed on this host.
- After a few runs, retune model and effort with `tune-fleet` from
  `horch cost` numbers.
