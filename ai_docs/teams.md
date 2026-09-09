# Teammates and recipes — design plan

> **Status: phases 1-3 are implemented and shipped.** `teammates/` is live,
> `prompts.rs` holds no prompt text, `tiers.rs` is deleted, and `horch
> teammates` exists. What is left below is phases 4-5 (composition and custom
> agents) plus the open questions. Sections describing "today" that have since
> been built are marked DONE.

## The question this answers

> Is there a way to describe what an agent should look like — start-up prompt,
> arguments, first-instruction prompt, persona — that the orchestrator could
> invoke?

**Today: no.** There is a *tier*, which is a much smaller thing. This document
plans the larger thing, and calls it a **teammate**; a named composition of
profiles is a **team**.

## What exists today

> **Historical.** This section describes the state before the roster landed and
> is kept because the rest of the document argues against it. `Tier` no longer
> exists; `teammates/` is the source of truth. DONE.

`horch` has exactly one axis of worker variation, and it is compiled in.

| Concept | Where | What it holds |
|---|---|---|
| `Tier` | `crates/horch-core/src/tiers.rs` | A closed 6-variant enum. Maps tier → agent (`Claude`/`Codex`/`None`) + model string. Nothing else. |
| Briefings | `crates/horch-core/src/prompts.rs` | `const` strings and `fn`s. `tier_briefing(Tier) -> &'static str` is a `match` over the enum. |
| Launch argv | `crates/horch/src/cmd/worker.rs` | `launch_claude` / `launch_codex` hard-code the flags, the env, and the resume syntax per agent. |
| Fleet shape | `crates/horch/src/cmd/recipes.rs` | `fleet()` hard-codes the 2x2 grid: sonnet, opus, sonnet, codex-sol. |

So "what an agent looks like" is spread across four files and three of them
require a recompile to change. A tier cannot express a persona, extra CLI
arguments, environment, a first-instruction template, or a startup prompt
override.

### The specific hard-codings a teammate file has to absorb

Collected from the current source, because these are the seams that will move:

- `worker.rs::launch_claude` — sets `CLAUDE_CODE_EFFORT_LEVEL=xhigh` for every
  Claude worker, and `CLAUDE_CODE_SUBAGENT_MODEL=haiku` for `Tier::Fable` only.
  Chooses `--session-id <id>` vs `--resume <id>`.
- `worker.rs::launch_codex` — passes `-c model="<model>"`, and resumes with the
  entirely different `resume -c model="..." <id>` form.
- `spawn.rs` — decides whether to mint a session id at all by asking
  `tier.agent() == Agent::Claude`. Codex mints its own after launch.
- `prompts.rs::tier_briefing` — the per-tier paragraph. This is the closest
  thing to a persona that exists.
- `codex.rs::ensure_rules` + `prompts.rs::codex_rules` — execpolicy rules that
  must be allowed for a codex worker to report back at all.

## Proposed model

Two things: **teammates** (one file each) and **teams** (named compositions of
them). Both are folders of markdown-with-YAML-frontmatter files, read at
runtime.

> **Superseded:** an earlier draft of this plan proposed a single
> `profiles.toml`. That is dropped. Reasons: personas are multi-paragraph
> prose and TOML is poor at that; `SKILL.md` and `.claude/agents/*.md` already
> use frontmatter-plus-body on this machine, so the convention is familiar;
> and one-file-per-teammate makes discovery literal — the file exists,
> therefore the orchestrator can pick it.

### Teammate files

**Already written: `teammates/`.** The schema is settled; only the loader is
outstanding. See `teammates/_template.md` for the annotated version and
`teammates/README.md` for the rules.

| Field | Purpose | Absorbs |
|---|---|---|
| filename / `name` | What `horch spawn <name>` takes | `Tier::from_str` |
| `brief_description` | **The only field the orchestrator reads** | the hard-coded tier list in `FLEET_ORCHESTRATOR` |
| `generic` | Marks the fallbacks, for roster grouping | nothing today |
| `hidden` | Loaded and spawnable, but kept out of the roster | the `Smoke` tier |
| `base` | Which `_base/*.md` protocol prompt to inherit | `fleet_worker()`'s fixed body |
| body | Persona: identity and scope | `tier_briefing()` |
| `agent` | Which CLI to launch | `Tier::agent()` |
| `model` | Alias (claude) or slug (codex) | `Tier::model()` |
| `effort` | `--effort` (claude) / `-c model_reasoning_effort` (codex) | hard-coded `xhigh` at `worker.rs:84` |
| `subagent_model` | `CLAUDE_CODE_SUBAGENT_MODEL` | the Fable special-case at `worker.rs:88` |
| `permission_mode` | `--permission-mode` (claude) / sandbox+approval pair (codex) | nothing today |
| `tools` / `allowed_tools` / `disallowed_tools` | `--tools` / `--allowedTools` / `--disallowedTools` | nothing today |
| `inherit_settings` / `settings` | `--restricted` / `--settings` | nothing today |
| `plugin_dirs` | `--plugin-dir`, repeatable | nothing today |
| `skills` | Prompt-level "invoke /x first" | nothing today |
| `args` / `env` | Escape hatch for anything unnamed | nothing today |
| `first_instruction` | Templated opening line | `task_briefing()` |

`effort` and `skills` are the two fields that look universal and are not.
Effort is a flag on Claude and a config key on Codex; skills do not exist on
Codex at all, and on Claude there is **no `--skill` flag** — skills are
discovered from plugin directories and invoked as `/name`. So `plugin_dirs`
is argv and `skills` is prompt text, and they have to stay separate fields.

### Startup mode and tools — the operator's settings leak into every worker

This is the sharpest gap the tier model has, and it is already live rather
than hypothetical. `~/.claude/settings.json` on this machine contains:

```json
"permissions": { "deny": ["ExitPlanMode", "NotebookEdit"] }
```

`launch_claude` (`worker.rs:83`) passes no `--permission-mode`, no `--tools`,
and no `--settings`, so every fleet worker inherits that deny. A teammate whose
whole job is planning would be started in plan mode and then be unable to leave
it — and it fails *silently*, in a pane nobody is watching, which is the worst
shape a failure can have in this system.

Three separate knobs, deliberately not collapsed into one:

| Field | Claude | Codex |
|---|---|---|
| `permission_mode` | `--permission-mode acceptEdits\|auto\|bypassPermissions\|manual\|dontAsk\|plan` | mapped to `-s <sandbox> -a <approval>` |
| `tools` | `--tools` — the *available* set | no equivalent; load-time error |
| `allowed_tools` / `disallowed_tools` | `--allowedTools` / `--disallowedTools` — pre-approved / denied within the available set | no equivalent; load-time error |
| `inherit_settings: false` | `--restricted` (ignores user/project/local settings) | n/a |
| `settings` | `--settings <path>` | n/a; codex has `-p <profile>` via `args` |

The claude→codex mapping for `permission_mode`:

| `permission_mode` | Codex flags |
|---|---|
| `plan` | `-s read-only  -a on-request` |
| `acceptEdits` | `-s workspace-write  -a on-request` |
| `auto` | `-s workspace-write  -a never` |
| `bypassPermissions` | `--dangerously-bypass-approvals-and-sandbox` |
| `manual`, `dontAsk` | no equivalent — load-time error, not a silent downgrade |

`--check` rules this implies:

- A `permission_mode: plan` teammate must be able to reach `ExitPlanMode`,
  by `inherit_settings: false` or a `settings` file without the deny.
  `allowed_tools` is **not** assumed to undo a settings-level deny — deny beats
  allow — so `--check` should probe rather than trust the field.
- `inherit_settings: false` strips Bash, the other code-running tools, and
  WebFetch unless `tools` names them. Setting one without the other is a
  worker that cannot edit anything; warn.
- `tools`, `allowed_tools`, `disallowed_tools`, `skills` on an `agent: codex`
  teammate: error. Codex controls capability by sandbox, not by tool list.

**The generics set `permission_mode: acceptEdits`.** This is the one place the
built-ins deliberately do *not* mirror today's behaviour: today the mode is
whatever the operator's settings default to, and for an unattended pane that is
a coin flip on whether the worker hangs. They leave `tools` unset, so the
default tool set still applies.

### Base prompts

`teammates/_base/*.md`, named by a teammate's `base:` field. The base owns the
`horch tell` / `horch note` / `horch done` protocol; the persona owns identity.
Splitting them means no hand-written persona can accidentally ship a worker
with no channel back to the orchestrator.

`_base/fleet-worker.md` is currently a **byte-exact mirror** of the format
string in `prompts.rs::fleet_worker` (verified). It is documentation until
phase 3, which flips ownership: the file becomes the source and the const is
deleted. Until then, edits must be made in both places or they will diverge.

### The `session` block — derived, not configured

An earlier draft made session handling a per-profile block. It should instead
be **derived from `agent`**, because it is a property of the CLI, not of the
teammate, and getting it wrong is unrecoverable:

| `agent` | id minted by | fresh | resume |
|---|---|---|---|
| `claude` | horch (caller) | `--session-id {session}` | `--resume {session}` |
| `codex` | codex (agent) | none; harvested from the rollout file | `resume -c model={model} {session}` |
| `none` | n/a | n/a | n/a |

`id_source = "agent"` is what tells `spawn.rs` to leave `session_id` empty and
`worker.rs` to start the background harvest. The rollout-file scan in
`codex.rs` is genuinely codex-specific and does not generalise, so it stays a
built-in strategy keyed on `agent` rather than a configurable hook. A
per-teammate override only becomes necessary at phase 5 (`agent = "custom"`),
and can be added then.

### Team composition - call it a recipe

`horch` already has *recipes* (`horch fleet`, `horch orchestration`): a named
way to stand up a whole workspace. A named composition of teammates is the same
idea with the roster behind it, so it should reuse the word rather than
introduce "teams" alongside it. Sibling folder, same file convention:
`recipes/<name>.md`.

```yaml
---
name: default
description: Balanced fleet - two cheap, one strong, one codex.
orchestrator: orchestrator
workers:
  - { teammate: sonnet,    count: 2 }
  - { teammate: opus,      count: 1 }
  - { teammate: codex-sol, count: 1 }
---
```

`horch fleet --recipe default` would pre-spawn that set. Note this is now
strictly opt-in: `horch fleet` starts the orchestrator **alone** and lets it
spawn what the work actually needs, which is the better default. A recipe is
for the case where you already know the shape of the team.

Layout stays the business of `layout.rs` / `balance.rs`: the recipe says
*what*, the grid planner says *where*. Do not put pane coordinates in a recipe
file.

## Where the files live

```
built-in            compiled fallbacks, identical to the current 6 tiers
~/.config/horch/teammates/     user-wide
<project>/teammates/           per-project   <- the folder in this repo
```

Merged in that order, by id, **field by field** — so a project can override
just `model` on `sonnet` without restating the persona. `$HORCH_TEAMMATES_DIR`
overrides the lot, for tests and one-offs.

Shipping the current six tiers as built-ins under their existing ids is what
makes this non-breaking: `horch spawn sonnet "task"` keeps working and every
ledger record ever written still resolves.

**This must be a runtime directory read, not `include_str!`.** The whole point
is that dropping a file in the folder changes what the orchestrator can pick;
baking the folder in at compile time would reintroduce exactly the rebuild
requirement that makes `prompts.rs` awkward to tweak today. That means the
loader has to *find* the folder from a worker pane whose cwd is the target
project, not this repo. Order: `$HORCH_TEAMMATES_DIR`, then `<project>/teammates/`,
then `~/.config/horch/teammates/`, then built-ins. The justfile should export
`HORCH_TEAMMATES_DIR` so a fleet launched from here always sees this folder.

## Orchestrator-facing surface

This is the actual point of the exercise. Today `FLEET_ORCHESTRATOR` contains a
hand-written list of five tiers. That list has to become generated, or
teammates are invisible to the only thing that would use them.

```bash
horch teammates                # human table: name, agent, model, brief_description
horch teammates --json         # machine-readable
horch teammates --check        # validate: name==filename, description <=120 chars,
                               #   base exists, agents on PATH, templates parse,
                               #   no skills: on a codex teammate
horch recipes                  # list recipes and their composition
horch spawn <teammate> "task"  # unchanged spelling; <teammate> replaces <tier>
horch fleet --recipe <name>    # pre-spawn a known team shape
```

`pane_launch` renders the orchestrator briefing with a `{roster}` placeholder
replaced by **`brief_description` lines only** — specialists first, then
generics. Teammates marked `hidden: true` are loaded and spawnable but
never rendered. Nothing else from a teammate file reaches the orchestrator's context.
That is the whole reason `brief_description` is capped: the roster is a
per-run, per-decision context cost, and it grows with every file added.

## Back-compat

| Concern | Handling |
|---|---|
| Existing ledgers store `tier: "codex-sol"` | Built-in teammate ids are the old tier names, so lookups resolve unchanged. |
| `Brief` is JSON on disk | Add `teammate: Option<String>` with `#[serde(default)]`, exactly as `claude_bin` was added. Old briefs load. |
| `Record.tier` | Keep the field and its name. Add `teammate` alongside it; when absent, fall back to `tier`. |
| `Tier` enum | Becomes a thin deprecated alias resolving to a built-in teammate id, then is deleted in a later pass. |

## Mechanical churn to expect

`Tier` is `Copy` and hands out `&'static str`. Teammates are runtime-loaded, so
signatures change shape:

```rust
fn tier_briefing(tier: Tier) -> &'static str        // before
fn persona(t: &Teammate) -> &str                    // after
fn fleet_worker(role, tier, task, resume) -> String  // before
fn fleet_worker(role, t: &Teammate, task, resume) -> String       // after
```

Every call site threads a `&Teammate` (or an id plus the registry) where
it threads a `Tier` today. That is the bulk of the work and it is boring; the
interesting work is the `session` block.

## Templating

Deliberately dumb. Literal `{name}` substitution, no expressions, no
conditionals, no loops. Available: `{role}`, `{task}`, `{model}`, `{session}`,
`{project_dir}`, `{record_id}`, `{plan_file}`, `{roster}` (orchestrator only).
An unknown placeholder is a load-time error, not a silent empty string —
otherwise a typo in a persona template produces a worker briefed with a hole in
it and nobody finds out until it misbehaves.

### Where `{plan_file}` comes from

The orchestrator briefing now instructs: *never summarise a task to a worker,
write the plan to a file and send the file reference.* Nothing in the CLI
carries that path today — `horch spawn` and `horch assign` take a `--task`
string only, so the orchestrator has to inline the path into the task text and
hope the worker reads it.

Proposal: `--plan <path>` on both `horch spawn` and `horch assign`. It

- populates `{plan_file}` for the teammate's `first_instruction` and persona,
- is stored next to `task` in the ledger record, so `horch sessions` can show
  which plan a worker is executing and a resume re-points at the same file,
- is validated at spawn time (exists, readable, non-empty). A worker briefed to
  read a plan that is not there is a worker that will improvise.

`--task` stays as the short one-line label; `--plan` is the body. With no
`--plan`, `{plan_file}` resolves to empty and any teammate whose templates
reference it fails the same load-time check as any other unbound placeholder.

## Phasing

1. **Registry.** DONE. `Teammate` type, frontmatter parser, directory scan with the
   `_`-prefix skip rule, built-ins matching the current six tiers, merge order,
   `horch teammates` and `horch teammates --check`. `Tier` still exists and
   aliases to a built-in. No behaviour change. The files in `teammates/` are
   already written; this phase is the code that reads them.
2. **Launch data.** DONE. Move `args`, `env`, `bin`, and the `session` block out of
   `worker.rs` into the teammate file. This is where `xhigh`, `haiku`, and both
   resume syntaxes stop being `match` arms. Highest-risk step; the smoke checks
   are the safety net.
3. **Prompts as data.** DONE. Personas and `_base/*.md` become the source of truth
   and the `prompts.rs` consts are deleted. Inject `{roster}` into the
   orchestrator briefing, replacing its hand-written tier list. The
   prompts↔execpolicy sync
   test in `prompts.rs` becomes a runtime validation in `--check`, since the
   commands can no longer be known at compile time.
4. **Recipes.** `recipes/<name>.md` and `horch fleet --recipe <name>`, as an
   opt-in alternative to the orchestrator spawning on demand. NOT DONE.
5. **Custom agents.** `agent = "custom"` with a full argv template, so an agent
   `horch` has never heard of can be a teammate.

## Open questions

- **Codex `on-request` still hangs.** `acceptEdits` maps to `-a on-request`,
  which lets the model decide when to ask — and an ask in an unwatched pane is
  the same silent stall as a claude permission prompt. Only `-a never` fully
  removes it. Should codex teammates default to `auto` instead, accepting a
  more permissive worker in exchange for one that cannot deadlock?
- **Who owns the worker settings file?** If `settings: <path>` is the answer to
  the `ExitPlanMode` deny, that file has to live somewhere and be kept in sync
  with the operator's real settings. Ship one at `teammates/_base/settings.json`
  and treat it as part of the repo, or generate it at spawn time by copying the
  operator's and stripping the denies a teammate declares it needs?
- **Generic naming.** The generics are `opus.md`, not `generic-opus.md`, so
  `horch spawn opus`, the ledger's `tier` field, and the tier table in
  `FLEET_ORCHESTRATOR` all keep working untouched. The cost is that "generic"
  lives in `brief_description` and the `generic: true` flag rather than in the
  name. If the prefix is wanted, add `aliases: [opus]` and accept that the
  ledger now stores a name nothing else uses.
- **Fable as a fifth generic.** Fable was not in the original four but is a
  current tier, and it is the *only* one with unique launch config
  (`CLAUDE_CODE_SUBAGENT_MODEL=haiku`). Dropping it would silently remove the
  planning tier and the one field that justifies `subagent_model`, so
  `fable.md` exists. Delete it if the planning role is meant to belong to the
  orchestrator alone.
- **Roster growth.** `brief_description` is capped at 120 chars, but nothing
  caps the *number* of teammates, and the roster is re-read on every spawn
  decision. Thirty specialists is ~3.5 KB of orchestrator context per run.
  Does the roster need paging, a per-team filter, or just a documented ceiling?

- **Per-teammate execpolicy.** Codex needs `horch tell|note|done` allowed before
  a worker can report at all. If a custom teammate forgets, the worker launches
  and is then mute — the worst failure mode in the system. Should `--check`
  refuse to spawn a codex-family teammate whose rules are not installed?
- **Model validation.** A typo'd model fails at agent launch, inside a pane
  nobody is watching. `--check` cannot verify a model name without calling the
  API. Accept the gap, or probe once and cache?
- **Team layout.** Teams above are counts only. If someone wants "opus always
  top-left", that is a layout concern leaking into composition. Recommend
  refusing it in v1.
- **Teammate inheritance.** `extends: sonnet` is tempting and cheap to
  add, but two-level merge (built-in → user → project → extends) gets confusing
  fast. Recommend deferring until someone actually asks.
- **Overlap with the Claude Code herdr skills.** This machine already has
  `herdr-orchestrator`, `herdr-worker`, and `herdr-atomic` skills
  (`~/.claude/plugins/marketplaces/matts-robot-skills/plugins/herdr/skills/`)
  that fire on the same triggers and brief the same two roles. A teammate's
  `persona` and `briefing` land *inside* an agent that may have already loaded
  one of those skills, so the two can contradict each other — and the skill
  wins, because it arrives as an instruction rather than as pane text. The plan
  needs a stated owner before phase 3 ships. Recommended split: the skill owns
  *protocol* (which `horch` subcommands to call, how to report, when to stop),
  a teammate owns *identity and task* (persona, model, task, plan file), and
  personas must not restate protocol. Otherwise the honest option is
  for `horch` to emit the skill files itself so there is one source.

- **Ledger schema.** Adding `teammate` alongside `tier` means two fields that
  usually agree, which invites drift. The alternative — reusing `tier` to hold
  a teammate id — is cleaner but silently changes what the field means for
  anything already reading those files.
