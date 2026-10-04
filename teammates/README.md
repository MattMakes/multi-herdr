# teammates/

One file per team member. **Adding a file adds an option the orchestrator can
choose from** — there is no registry to update and nothing to recompile.

> **These files are live.** They are the only place agent briefings exist.
> `crates/horch-core/src/prompts.rs` holds no prompt text; it reads these files
> and substitutes `{placeholder}` spans, nothing more. `tiers.rs` is gone.
>
> ```bash
> horch teammates            # who the orchestrator can pick
> horch teammates --check    # validate before a fleet reads them for real
> horch teammates --new qa   # scaffold qa.md from _template.md
> ```
>
> Edits take effect on the next `just herdr-fleet` with no rebuild, because the
> justfile exports `HORCH_TEAMMATES_DIR` and the roster is read at run time. A
> copy is also compiled in, so an installed `horch` binary works with no repo
> checked out.

## Rules

- **Filename is the id.** `opus.md` → `horch spawn opus`. `name:` in the
  frontmatter must match, and a mismatch is a load-time error rather than a
  quietly-wrong roster.
- **A leading `_` means "not parsed."** `_template.md` and `_base/` are not
  teammates and are never read as such.
- **`hidden: true` means "loaded, but not offered."** The teammate never
  appears in the roster. `smoke.md` uses this, so `horch smoke fleet` keeps
  working without costing the orchestrator a roster line for a fake agent; the
  two orchestrators use it because they are launched into a pane, never spawned
  into one.
- **The top tier is reserved for the orchestrator.** `fable` and `gpt-6-astra`
  are refused by `horch spawn`, whichever flavor is orchestrating: a fleet has
  at most one top-tier session (none under `horch fleet opus` or `sol`). The
  rule is matched on the tier name, so a version bump stays reserved. `horch teammates --check` fails any offered
  teammate that asks for one.
- **`brief_description` is the only field the orchestrator reads** to decide
  between a specialist and a generic. One line, ≤120 characters, enforced at
  load. Everything else — persona, model, effort, skills — is applied when the
  teammate is spawned and is never in the orchestrator's context.
- **Be explicit about mode and tools.** Anything left unset is inherited from
  the operator's `~/.claude/settings.json`, which is written for an attended
  session, not for a worker in a pane nobody is watching. On this machine that
  file denies `ExitPlanMode`, so a plan-mode teammate that says nothing about
  tools cannot leave plan mode. See the `permission_mode` / `tools` block in
  `_template.md`.
- **Personas do not carry protocol.** `_base/fleet-worker.md` owns the
  `horch tell` / `horch note` / `horch done` contract, and
  `_base/fleet-orchestrator.md` owns the spawning and ledger contract; a
  persona that restates either will drift from it. `orchestrator.md` and
  `orchestrator-codex.md` are personas in exactly this sense: each is only the
  paragraph about writing for the workers below it, substituted into the one
  shared briefing at `{persona}`.
- **No fleet pane spawns subagents.** A subagent's work never reaches the
  ledger or the grid, and splitting the work is the orchestrator's decision,
  not a worker's. A worker that needs more hands sends `QUESTION:` to the
  orchestrator. Prose alone did not hold this, so the switch carries it: every
  `agent: claude` teammate the orchestrator can spawn, and the orchestrator
  itself, sets `disallowed_tools: [Agent]`, and `horch teammates --check` fails
  one that does not. `allow_subagents: true` waives the check for one teammate
  and denies nothing by itself; nothing shipped sets it. The Codex teammates
  carry the same rule as `args: ["-c", "features.multi_agent=false"]`.
  `ai_docs/reports/no-subagents.md` has the evidence per harness.
- **The orchestrator delegates only to workers.** The Claude orchestrator
  also denies `RemoteTrigger`, which starts a cloud agent, and
  `allow_subagents` cannot waive either deny for it. Its briefing forbids a
  nested agent CLI (`claude -p`, `codex exec`) started from a shell.
- **Some capabilities are the orchestrator's alone.** The bundled
  `orchestrate` and `skill-creator` skills and `remote_control: true` are
  refused by `horch teammates --check` on any other teammate. Every Claude
  pane switches off the ambient skill-creator copies (the official plugin and
  the claude.ai-synced skill) and states `remoteControlAtStartup`, `false`
  except on the orchestrator. The Claude orchestrator starts in
  `permission_mode: auto`.
- **Agent-to-agent messages use Simplified Technical English (STE).** The rule
  lives in `_base/fleet-worker.md` and `_base/fleet-orchestrator.md`.

## The team

`product-lead`, `researcher`, `staff-engineer`, `designer`,
`architect-reviewer`, `frontend-developer`, `backend-developer` and
`qa-engineer` are the specialists. Their `phase` selects a small portable
skill catalog embedded in horch; `skills` adds named bundled skills for that
role. Skills are discovered by each harness and read only when relevant.
Claude specialists set `inherit_plugins: false` to switch off globally enabled
plugins while preserving other operator settings and their declared MCP tools.

Researcher, product-lead and designer default to `research`; staff-engineer and
orchestrators to `plan`; reviewers and QA to `validation`; other workers to
`implementation`. Override a task with `horch spawn opus --phase research`.
Resume preserves the recorded phase unless `--phase` overrides it; old records
without a phase use the current teammate default. A phase selects guidance,
not permission mode or tool access.

## The design team

Six more specialists cover visual design. Each carries bundled design skills
by name, not by phase, so `--phase` never hands them to another teammate.

| teammate | phase | agent, model, effort | MCP servers | use for |
|---|---|---|---|---|
| `design-director` | research | claude, opus, high | none | direction contract and storyboard; no production code |
| `design-critic` | validation | claude, opus, high | playwright | scored review with screenshots; never edits code |
| `landing-page-builder` | implementation | claude, opus, medium | playwright, chrome-devtools, context7 | landing and marketing pages |
| `design-system-engineer` | implementation | claude, sonnet, medium | playwright, context7 | tokens, themes, components, brand kit |
| `motion-engineer` | implementation | claude, opus, medium | playwright, chrome-devtools, context7 | GSAP scroll scenes and transitions |
| `visual-prototyper` | implementation | codex, gpt-5.6-sol, medium | none | generated images and a static prototype |

`design-critic` denies `Edit` and `NotebookEdit` but keeps `Write` for its one
critique file. Every builder takes screenshots at 390, 768 and 1440 px before
it reports done. `visual-prototyper` uses Codex image generation when the
session has it, and writes prompts and placeholders when it does not.
`design-system-engineer` runs on sonnet because the direction contract
supplies the judgement; spawn `opus` for a system with no direction. The
orchestrator's pipeline for these seats is in `skills/orchestrate/SKILL.md`
under "Design work".

Inspect catalogs and context estimates with `horch skills --phase validation`.
The built-in roster requires no local plugin paths. Custom Claude teammates can
still declare `plugin_dirs` (`~/` expands at launch); `horch teammates --check`
verifies those paths exist. `disable_skills` conflicts with phase, skills, or
plugin directories.

## The Unreal Engine team

Eleven `ue-*` specialists staff Unreal Engine work. Each sets
`offer_when: ["*.uproject"]`, so the fleet orchestrator is offered them only
when the project has a `.uproject` file at the top level or one level down.
`horch spawn ue-<name>` still works in any project. The selection, the
skill choices and the measured briefing sizes are in
[`ai_docs/reports/unreal-engine-wave.md`](../ai_docs/reports/unreal-engine-wave.md)
and [`ai_docs/reports/domain-skills/ue-teammates.md`](../ai_docs/reports/domain-skills/ue-teammates.md).

| teammate | wave | phase | agent, model, effort | use for |
|---|---|---|---|---|
| `ue-tech-lead` | 1 | plan | claude, opus, high | runs first: writes `.agents/ue-project-context.md`; plans modules and plugins |
| `ue-gameplay-engineer` | 1 | implementation | claude, opus, medium | actors, components, gameplay framework, GAS, tags, data assets |
| `ue-network-engineer` | 1 | implementation | claude, opus, medium | replication, RPCs, prediction, net movement, dedicated servers |
| `ue-technical-artist` | 1 | implementation | claude, opus, medium | materials, Niagara, audio, Sequencer, procedural content |
| `ue-qa-engineer` | 1 | validation | claude, sonnet, high | headless builds, automation tests, crash repros, profiling |
| `ue-code-reviewer` | 1 | validation | claude, opus, high | UE C++ review; never edits |
| `ue-character-engineer` | 2 | implementation | claude, opus, medium | movement modes, animation, cameras, Enhanced Input |
| `ue-ai-engineer` | 2 | implementation | claude, opus, medium | AI controllers, behavior trees, EQS, State Tree, Mass |
| `ue-ui-engineer` | 2 | implementation | claude, opus, medium | UMG, Slate, Common UI, MVVM, gamepad focus |
| `ue-tools-engineer` | 2 | implementation | claude, opus, medium | editor modules, customizations, validators, editor scripts |
| `ue-world-engineer` | 2 | implementation | claude, opus, medium | World Partition, streaming, PCG, collision, save games |

Every one runs on Claude: native Windows refuses a Codex teammate that has
skills, and most Unreal work happens on Windows. Only `ue-tech-lead` and
`ue-code-reviewer`, which read and plan but never build, fall back to
`codex-sol`. The builders and `ue-qa-engineer` have no fallback: a Codex pane
runs with the network off and `workspace-write`, which may block the shared
Derived Data Cache, and nobody has tried it on a UE project yet. When the
Claude pool is out, `horch route` refuses and the orchestrator waits. Add a
Codex fallback after a trial on a real project. Every implementation
teammate and `ue-qa-engineer` carry `ue-build-verify`. `ue-technical-artist`
and `ue-tools-engineer` also carry `ue-editor-scripting`, and so they may
change an asset through editor Python or a commandlet. The others never
touch `.uasset` or `.umap` files and list the asset change for a human.
`available_skills:` names at most 3 related skills per teammate, picked from
the "Related Skills" sections of its own skills.

Each persona carries the same five standing rules: read
`.agents/ue-project-context.md` first, never write asset bytes, check APIs in
the engine headers, one build at a time per working copy, and a `DONE:` that
names the target, the configuration and the automation filter with its
result.

## The effort-matrix personas

Eight more Claude specialists split the work by model tier and effort level
rather than by discipline. Sonnet does fast, cheap execution. Opus 5.5 brings
more baseline reasoning and handles complex abstraction. The effort level sets
how deep each one checks.

| effort | Sonnet | Opus |
|---|---|---|
| low | `sonnet-sketch`: drafts, brainstorms, boilerplate, one-file edits, scoping | `opus-architect`: architecture critique, API and schema design, multi-service bug triage |
| medium | `sonnet-feature`: endpoints, UI components, CRUD, test updates to a clear spec | `opus-domain`: intricate algorithms, domain rules, brownfield cross-cutting features |
| high | `sonnet-bugfix`: localized bugs with repro steps, edge-case validation, module refactors | `opus-hardening`: races, leaks, concurrency, adversarial security review, fuzz harnesses |
| max | `sonnet-sweep`: one refactor over many files, e2e boilerplate from a schema | `opus-verify`: Lean 4, hardware synthesis, compiler passes, sandboxed pentest |

Each persona states its boundary ("stop and report when...") in its body, and
the short form of that boundary in `brief_description`, so both the worker and
the orchestrator see it. The two `max` seats, `sonnet-sweep` and `opus-verify`,
have a `first_instruction` that makes the worker ask and wait on an
underspecified task. At max effort a wrong premise costs the most tokens.
Like the other Claude seats, each has a Codex `fallbacks:` entry for when the
Claude pool cannot serve a spawn: `codex-sol` for the Opus four, `codex-terra`
for the Sonnet four (`horch route <name>` shows the decision).

Token efficiency: for deep reasoning, `opus-hardening` (Opus at high) often
matches or beats Sonnet at max on fewer tokens. `sonnet-sweep` exists for
repetitive volume, not for depth. `opus-architect` is the fast, low-effort
counterpart of `staff-engineer` (deep plans, high) and `architect-reviewer`
(reviews a change, high). Rationale per seat is in
[`ai_docs/reports/model-guide-2026-09.md`](../ai_docs/reports/model-guide-2026-09.md).

## The generics

`sonnet`, `opus`, `codex-sol`, `codex-network`, `codex-terra`, `codex-luna`, `opencode-ultra`,
`opencode-pickle`, `opencode-lightning`, `pi` and `prime` are the fallbacks:
what the orchestrator spawns when no specialist's `brief_description` matches
the work. They are named after the tier or the harness on purpose, so
`horch spawn opus`, the ledger's `tier` field, and the roster in the
orchestrator briefing all keep meaning the same thing. There is no `fable` and
no `astra` generic: those tiers belong to the orchestrator.

`codex-network` is the only Codex worker whose sandbox reaches the network
(`-c sandbox_workspace_write.network_access=true`). Every other Codex
teammate keeps the network off, because it runs `auto` (`-a never`) and
nothing would stop it from sending code out. `codex-network` runs
`acceptEdits` (`-a on-request`) instead, and `horch teammates --check` fails
any Codex teammate that pairs network access with `auto` or
`bypassPermissions`. Claude workers need no switch: no Claude sandbox is
configured, so their Bash and WebFetch already reach the network.

They are not one ladder but three, and the orchestrator is choosing on cost and
confidentiality as much as on capability:

| | pays with | send it |
|---|---|---|
| `sonnet` `opus` `codex-*` `prime` | money | anything the project already trusts these providers with |
| `opencode-*` | **your prompts** | public and open-source work only |
| `antigravity` | **your prompts**, until the operator confirms the opt-out | public and open-source work only |
| `pi` | your own GPU | anything, including what must not leave the machine |

## Effort: set by role, stated in every file

Each teammate's `effort:` follows its role, not its model
([ImCesar/cezaar#40](https://github.com/ImCesar/cezaar/issues/40)). The
evidence per teammate, with prices and sources, is in
[`ai_docs/reports/model-guide-2026-09.md`](../ai_docs/reports/model-guide-2026-09.md).

| role | teammates | effort |
|---|---|---|
| orchestrator | `orchestrator`, `orchestrator-codex` | xhigh |
| reviewers and planners | `architect-reviewer`, `qa-engineer`, `codex-reviewer`, `staff-engineer`, `product-lead` | high |
| builders and research | `backend-developer`, `frontend-developer`, `designer`, `researcher`, `sonnet`, `opus`, `codex-sol`, `codex-network`, `prime` | medium |
| runners | `codex-terra`, `codex-luna`, `pi` | low |
| effort-matrix personas | `sonnet-*`, `opus-*` (effort is the role; see above) | low to max |
| no effort setting | `opencode-*` (their free models define no variants) | - |

Rules `horch teammates --check` enforces:
- the level must be one this agent's CLI takes;
- an offered codex teammate must state one;
- haiku and the free opencode models must not have one.

Change a level with the `tune-fleet` skill (`.claude/skills/tune-fleet/`),
which works from `horch cost` numbers, and write the reason next to the line.

## Free tiers and `trains_on_input`

The `opencode-*` teammates run on OpenCode's free models. Free means the
provider trains on what it is sent, so every one of them sets
`trains_on_input: true`. That appends the shared block from
`_base/fleet-worker.md` to the worker's briefing: treat the pane as public,
work only with public or open-source code, and report `BLOCKED` rather than
read anything proprietary into it.

Set the flag on any teammate whose provider trains on input, and say so in
`brief_description` too - the flag reaches the worker, the description reaches
the orchestrator deciding who gets the task, and the decision needs both ends.
Leave it off for paid, local and self-hosted models: a warning that appears
everywhere stops being read anywhere.

## Running `pi` on local models

`pi` needs an Ollama provider before `ollama/qwen3.8` resolves. Add
`~/.pi/agent/models.json`:

```json
{
  "providers": {
    "ollama": {
      "baseUrl": "http://localhost:11434/v1",
      "api": "openai-completions",
      "apiKey": "ollama",
      "models": [
        {
          "id": "qwen3.8",
          "name": "Qwen3.8 27B (local)",
          "reasoning": true,
          "input": ["text", "image"],
          "contextWindow": 262144,
          "maxTokens": 32000,
          "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }
      ]
    }
  }
}
```

`pi --list-models | grep ollama` confirms it. The `id` must match the Ollama tag
(`ollama list`), and `pi.md`'s `model:` must match `ollama/<id>`.

`pi.md` runs at `effort: low`. On Qwen3.8 the long-context benchmark scores
`xhigh` below `low`, and a local model pays for every thinking token in
wall-clock time. Known Qwen3.8 issues (2026-09):
- **`--thinking off` does not turn thinking off** without a
  `thinkingLevelMap` in the model entry.
- **The OpenAI-compatible endpoint (`/v1`) can hang on tool calls** with
  `qwen3.8:27b`. If a pi pane stalls mid-tool-call:
  1. Point the provider at Ollama's native API instead.
  2. Or switch the tag to `qwen3.6:27b` (SWE-bench Verified 77.2; fits
     24 GB). Update both `id` here and `pi.md`'s `model:`.

See `ai_docs/reports/model-guide-2026-09.md`.

## The two orchestrators

`orchestrator.md` (Claude Code) and `orchestrator-codex.md` (Codex) back the
four fleet flavors: `horch fleet opus` (the default) and `fable` launch the
first, `astra` and `sol` the second. The flavor passes the model as the pane's
`--model`, which wins over the file's own `model:`; the file's value is only
the fallback. They share one briefing — `_base/fleet-orchestrator.md` — and
each file is only the paragraph about writing for the workers below it. Change
how orchestration works in the base; change how one agent's orchestrator talks
in its file.

Both also carry `skills: [orchestrate]`. That is the fleet playbook — how to
decompose work, the plan-file template, how to choose a teammate, how to verify
a `DONE:`, how to wrap up. It is attached **by name, not by phase**: adding it
to the `plan` catalog in `crates/horch-core/src/skills.rs` would hand it to
`staff-engineer` and to anything else spawned with `--phase plan`, none of
which directs a fleet. A skill that arrives where it does not apply is context
spent to no effect, and an instruction a worker may act on. `orchestrate` has
no upstream; it was reconciled from the retired external `herdr-orchestrator`
and `herdr-worker` skills against the real CLI, and every command in it is one
`horch --help` prints.

The Codex one also needs `_base/codex-orchestrator-execpolicy.md`. Codex refuses
any command outside its sandbox that its rules file does not name, and the
worker rules allow `note` and `done` — nothing an orchestrator runs. Without the
orchestrator rules that pane comes up able to think and unable to spawn.
`horch teammates --check` fails a rule that no briefing asks for, so the
allowlist cannot quietly outgrow what the prompt actually uses.

Each set reaches only the pane that needs it: horch builds a private
`CODEX_HOME` per launch rather than appending to the operator's shared
`~/.codex/rules/default.rules`. See "Starting codex panes clean" in the root
README.

## Adding a specialist

Copy `_template.md`, rename it, drop `generic:`, and write a
`brief_description` that answers "when would I reach for this?" — the
orchestrator is choosing from that line alone.
