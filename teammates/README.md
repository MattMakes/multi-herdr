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
  are refused by `horch spawn`, whichever of them is orchestrating: a fleet has
  exactly one top-tier session. The rule is matched on the tier name, so a
  version bump stays reserved. `horch teammates --check` fails any offered
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
  paragraph naming the tier it is the only session of, substituted into the one
  shared briefing at `{persona}`.
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

Inspect catalogs and context estimates with `horch skills --phase validation`.
The built-in roster requires no local plugin paths. Custom Claude teammates can
still declare `plugin_dirs` (`~/` expands at launch); `horch teammates --check`
verifies those paths exist. `disable_skills` conflicts with phase, skills, or
plugin directories.

## The generics

`sonnet`, `opus`, `codex-sol`, `codex-terra`, `opencode-ultra`,
`opencode-pickle`, `opencode-lightning`, `pi` and `prime` are the fallbacks:
what the orchestrator spawns when no specialist's `brief_description` matches
the work. They are named after the tier or the harness on purpose, so
`horch spawn opus`, the ledger's `tier` field, and the roster in the
orchestrator briefing all keep meaning the same thing. There is no `fable` and
no `astra` generic: those tiers belong to the orchestrator.

They are not one ladder but three, and the orchestrator is choosing on cost and
confidentiality as much as on capability:

| | pays with | send it |
|---|---|---|
| `sonnet` `opus` `codex-*` `prime` | money | anything the project already trusts these providers with |
| `opencode-*` | **your prompts** | public and open-source work only |
| `pi` | your own GPU | anything, including what must not leave the machine |

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

## The two orchestrators

`orchestrator.md` (Claude Code on Fable) and `orchestrator-codex.md` (Codex on
Astra) are what `horch fleet` and `horch fleet codex` launch. They share one
briefing — `_base/fleet-orchestrator.md` — and each file is only the paragraph
naming the tier it is the only session of. Change how orchestration works in
the base; change how one flavor talks about its own model in the file.

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
