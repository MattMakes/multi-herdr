# Bake-in orchestration: inventory and reconciliation

Date: 2026-09-20. Worker: `opus-1`. Plan: `ai_docs/plans/bake-in-orchestration.md`.

The operator's machine carries two symlinks under `~/.claude/skills`:

| symlink | target | size |
|---|---|---|
| `herdr-orchestrator` | `/Users/mascott/projects/matts-robot-skills/plugins/herdr/skills/herdr-orchestrator` | `SKILL.md` 228 lines / 14433 bytes, plus `references/layout.md` 61 lines and `references/tiers.md` 122 lines |
| `herdr-worker` | `/Users/mascott/projects/matts-robot-skills/plugins/herdr/skills/herdr-worker` | `SKILL.md` 115 lines / 6614 bytes |

Claude Code loads every `~/.claude/skills` entry into every fleet pane. Both
descriptions trigger on "orchestrator", "herdr", "horch", so a pane invokes
them. Their content is stale for this horch. This report reconciles every item
in them against the real CLI, which is pasted verbatim in
[Verified against the CLI](#verified-against-the-cli).

`decision` is one of `keep` (already correct and already in a `_base` file),
`carry` (correct guidance, not yet in the repo), `replace` (wrong command or
claim; carry the idea with the real command), `drop` (wrong or obsolete; do
not carry).

## Baseline and budget

Step 1 rule: a `_base` file gets only what a pane needs on every run, at most
15 added lines each. Everything longer goes to `skills/orchestrate/SKILL.md`.

| file | bytes before |
|---|---|
| `teammates/_base/fleet-orchestrator.md` | 6023 |
| `teammates/_base/fleet-worker.md` | 4077 |

The after numbers are in [Measurements (Step 6)](#measurements-step-6): 7041 and 4836.

## Table 1. `~/.claude/skills/herdr-orchestrator/SKILL.md`

| item | in the external skill | real horch equivalent | decision | destination |
|---|---|---|---|---|
| Role statement | "You direct a fleet of independent CLI agent sessions... Delegate aggressively." | Same text in `== Protect your context ==` and the lifecycle close. | keep | none |
| No AI-to-AI protocol | "Workers are plain terminal sessions... the only channel is text typed into terminals." | `== Channel ==` says the same. | keep | none |
| Rule 1, task goes in a file | "Every task goes into a briefing FILE before it goes to a worker." | Correct. The briefing is a repo file under `ai_docs/plans/`, and the spawn task text is `Read and follow <path> exactly.` | carry | `fleet-orchestrator.md` |
| Rule 1, tool enforcement | "`horch assign` and `horch spawn` accept `--brief <file>` and *reject* inline task text." | False. `horch spawn <TEAMMATE> [TASK]` and `horch assign <ROLE> <TASK>...` take positional task text. No `--brief` flag exists. | replace | `skills/orchestrate/SKILL.md` |
| Rule 2, one task per worker | "A worker exists for exactly one task... do not hand a second task to a worker that already finished one." | Correct as a practice. | carry | `skills/orchestrate/SKILL.md` |
| Rule 2, retire | "you retire it (`horch retire`), which closes its pane and frees its grid slot." | No `horch retire`. The worker runs `horch done`, which records the summary, reports DONE, and closes its own pane. | drop | none (already in `== Worker lifecycle ==`) |
| Launcher | "A human ran `horch \"<goal>\"` in their project." | `horch fleet` / `horch fleet codex`. | replace | `skills/orchestrate/SKILL.md` |
| Standing brief location | "It wrote your standing brief to `.herdr-orchestrator/ORCHESTRATOR.md`." | Nothing in `crates/` writes that file. The briefing is `teammates/_base/fleet-orchestrator.md` plus the persona. | drop | none |
| Ledger survives runs | "The space is new; the ledger is not... Run `horch sessions` before you spawn anything." | `horch sessions` exists and `== Session ledger: resume vs fresh ==` already says this. | keep | none |
| Inherited pending tasks | "Any worker left running in a previous space was auto-retired... `horch board` may show ready tasks you did not create." | No task board, no auto-retire. | drop | none |
| First command | "You never call `herdr` directly. Start with `horch status`." | No `horch status`. The first command is `horch sessions`. `horch inbox` lists live roles; `horch layout` reports the grid. | replace | `fleet-orchestrator.md` |
| `horch status` | "The grid, the board, and what is live. Your first command." | Does not exist. Split across `horch sessions`, `horch inbox`, `horch layout`. | replace | `fleet-orchestrator.md` |
| `horch sessions` | "The ledger... Read before every spawn decision." | Exists. `horch sessions [--json]`. | keep | none |
| `horch board` | "Task list, who owns what, what is ready now, and file-ownership conflicts." | Does not exist. The orchestrator owns conflict detection itself by giving each unit disjoint files in its plan file. | replace | `skills/orchestrate/SKILL.md` |
| `horch inbox` | "Live roles and their current herdr status." | Exists. `horch inbox` lists the roles registered in this workspace. | keep | none |
| `horch brief new <slug>` | "Scaffold a briefing file from the template." | Does not exist. The orchestrator writes `ai_docs/plans/<slug>.md` itself. | replace | `skills/orchestrate/SKILL.md` |
| `horch task add --title --brief --files --depends` | "Register a task. Prints its id." | Does not exist. | drop | none |
| `horch task set <id> --status` | "Close out a task you finished yourself." | Does not exist. | drop | none |
| `horch spawn ... --brief --role --task --effort` | Spawn flags. | Real: `horch spawn <TEAMMATE> [TASK]` with `--phase`, `--role`, `--from-pane`, `--direction`, `--no-tile`. No `--brief`, `--task`, `--effort`. | drop | none (already in `== Spawning workers ==`) |
| `horch spawn --brief <file> --resume <session-id>` | Resume form. | Real: `horch spawn --resume <ID> [TASK]`. | drop | none (already in `== Spawning workers ==`) |
| `horch assign <role> --brief <file> --task T` | "Give a briefing to a live idle worker." | Real: `horch assign <ROLE> <TASK>...`. | drop | none (already in `== Channel ==`) |
| `horch tell <role> "<answer>"` | "Short follow-ups and answers only (400 char cap)." | `horch tell <ROLE> <MESSAGE>...` exists. There is no 400-character cap in the source. | drop | none (already in `== Channel ==`) |
| `horch peek <role>` | "Read a worker's screen." | Does not exist. Use `horch inbox` and `horch sessions`. | drop | none |
| `horch retire <role> [--summary]` | "Mark done, close the pane, reflow the grid." | Does not exist. `horch done` is the worker's own command. | drop | none |
| `horch layout [--rebalance]` | "Show the grid and the next split; `--rebalance` re-equalizes it." | `horch layout` exists and takes `--pane` / `--workspace`, not `--rebalance`. `horch balance` equalizes columns. `horch tile` rebuilds the grid. | drop | none (already in `== Spawning workers ==`) |
| `horch tiers` | "Run `horch tiers` for the live catalog with launch commands." | Does not exist. The roster is `teammates/*.md`; `horch teammates` lists it and `{roster}` is substituted into the briefing. | replace | `skills/orchestrate/SKILL.md` |
| Tier table, `haiku` | A `haiku` tier. | No such teammate. | drop | none |
| Tier table, `fable` spawnable | "`fable` ... Planning, research, design." listed as a spawnable tier. | `fable` and `gpt-6-astra` are reserved for the orchestrator; `horch spawn` refuses them. | drop | none |
| Tier table, the rest | `sonnet`, `opus`, `codex-terra`, `codex-sol`, `pi`. | Real teammates, plus `prime`, `opencode-ultra`, `opencode-pickle`, `opencode-lightning` and 8 specialists. | replace | `skills/orchestrate/SKILL.md` |
| "The tier picks the mind; the effort picks how hard it thinks." | Two separate choices. | Half true. The teammate is a choice. Effort is fixed per teammate in its frontmatter; there is no `--effort` flag. | replace | `skills/orchestrate/SKILL.md` |
| Effort ladder table | `minimal`/`low`/`medium`/`high`/`xhigh`/`max` per spawn. | No per-spawn effort. | drop | none |
| "Lower one level when your briefing already specifies the change precisely" | A good spec substitutes for reasoning. | Correct and still the cheapest lever, restated without `--effort`: a precise plan lets a lower teammate do the work. | replace | `skills/orchestrate/SKILL.md` |
| `tiers.json` per-project overrides | `<project>/.herdr-orchestrator/tiers.json`. | Does not exist. `teammates/*.md` is the registry; adding a file adds an option. | drop | none |
| Auto mode / sandbox launch claims | "Claude workers always launch in auto mode... Codex with `--sandbox workspace-write`." | Set per teammate in frontmatter (`permission_mode`), not a fleet-wide rule the orchestrator acts on. | drop | none |
| Core loop 1, survey | "Read the project enough to decompose the work." | Correct and missing from the base. | carry | `fleet-orchestrator.md` |
| Core loop 2, check the ledger | "`horch sessions` ... Then `horch board` and `horch inbox`." | `horch sessions` and `horch inbox` exist; `horch board` does not. | replace | `fleet-orchestrator.md` |
| Core loop 3, decompose | "Decompose into independent, ownable units... Where files overlap, partition or serialize." | Correct. Without `horch task add --files`, the plan file's `FILES` section is where ownership is declared. | replace | `fleet-orchestrator.md` |
| Core loop 4, write the briefing file | "One per task, using the template below. Then spawn." | Correct. Path is `ai_docs/plans/<slug>.md`. | carry | `fleet-orchestrator.md` |
| Core loop 5, maximize parallelism | "Keep that many workers busy... don't drip-feed one at a time." | Correct, minus `horch board`. Spawn every conflict-free unit at once. | replace | `fleet-orchestrator.md` |
| Core loop 6, monitor | "A blocked worker asks and then waits... Answer with `horch tell` promptly." | Correct. | carry | `fleet-orchestrator.md` |
| Core loop 7, verify | "read the summary, verify it yourself (run the tests, inspect the diff), then `horch retire`." | Verify yourself: correct. `horch retire`: does not exist; the worker already closed its own pane. | replace | `fleet-orchestrator.md` |
| Core loop 8, shrink | "When remaining work no longer justifies the fleet, stop spawning." | Correct. | carry | `fleet-orchestrator.md` |
| Core loop 9, wrap up | "`horch board` shows every task completed... `horch inbox` shows no live workers still mid-task." | `horch inbox` part is correct; `horch board` does not exist. | replace | `fleet-orchestrator.md` |
| Briefing template | `GOAL / CONTEXT / FILES / PRIOR WORK / CONSTRAINTS / DONE WHEN / REPORT`. | Correct shape. This repo also wants `STEPS` with a check per step. | carry | `skills/orchestrate/SKILL.md` |
| Briefing validation | "`GOAL:`, `FILES:` and `DONE WHEN:` are required - `horch` refuses a briefing without them." | False. No command reads a plan file. | drop | none |
| Resume vs fresh | "Default to a fresh spawn and carry knowledge forward explicitly." | Correct. `== Session ledger: resume vs fresh ==` already says this at length. | keep | none |
| Resume question test | "Could I write down what it knows in ten lines? If yes, spawn fresh." | Correct and compact. | carry | `skills/orchestrate/SKILL.md` |
| Layout, 2xN grid | "orchestrator left at full height, workers in a 2-row x N-column grid." | Correct; `horch tile` builds exactly this across tabs. | keep | none |
| Layout, no manual splits | "`horch spawn` picks the split... you do not run `herdr pane split` yourself." | Correct. `== Spawning workers ==` already says it. | keep | none |
| Worker message handling | "`[<role>]` lines are worker messages, not your human operator. Never reply in plain text; use `horch tell`." | Correct. `== Channel ==` already says it. | keep | none |
| `[<role>] ready` / `BLOCKED:` / `DONE:` | The three message shapes. | Correct; `== Worker lifecycle ==` already lists them. | keep | none |
| Silent role | "`horch inbox`, `horch peek <role>`, `horch sessions`." | `horch peek` does not exist; the other two do. | drop | none (already in `== Worker lifecycle ==`) |
| Goes wrong, typing a task | "If you catch yourself composing a task in a `horch tell`, stop: write the file." | Correct as practice; the "`tell` rejects briefing-shaped text" reason is false. | replace | `skills/orchestrate/SKILL.md` |
| Goes wrong, overlapping ownership | "Two workers touching the same file." | Correct. | carry | `skills/orchestrate/SKILL.md` |
| Goes wrong, under-specified briefings | "'Fix the auth bug' produces three questions or a wrong guess." | Correct. | carry | `skills/orchestrate/SKILL.md` |
| Goes wrong, hoarding work | "If you're writing substantial code while workers sit idle, stop and delegate." | Correct. | carry | `skills/orchestrate/SKILL.md` |
| Goes wrong, under-parallelizing | "Running one worker at a time when four tasks are conflict-free." | Correct, minus `horch board`. | replace | `skills/orchestrate/SKILL.md` |
| Goes wrong, spawning before the ledger | "You'll duplicate finished work or lose gotchas." | Correct. | carry | `skills/orchestrate/SKILL.md` |
| Goes wrong, reusing a finished worker | "Retire it and spawn fresh with PRIOR WORK." | Correct minus `retire`: the pane is already closed; spawn fresh with PRIOR WORK. | replace | `skills/orchestrate/SKILL.md` |
| Goes wrong, `[role]` lines as human input | "They aren't. They are status." | Correct. | carry | `skills/orchestrate/SKILL.md` |
| Goes wrong, forgetting the fleet | "Declaring done while three panes are mid-task." | Correct, checked with `horch inbox`, not `horch board`. | replace | `skills/orchestrate/SKILL.md` |
| Closing, protect your context | "Protect your context like a rare resource... NEVER do work yourself. Spawn workers." | `== Protect your context ==` already says this. | keep | none |

## Table 2. `~/.claude/skills/herdr-orchestrator/references/layout.md`

| item | in the external skill | real horch equivalent | decision | destination |
|---|---|---|---|---|
| Grid shape | "Orchestrator pinned left, full height; workers in a 2-row x N-column grid." | Correct. `horch tile` places the orchestrator full height on the left of tab 1, 4 workers beside it in 2x2, 6 per overflow tab in 2x3. | keep | none |
| `horch init --ratio` | Sets the orchestrator's width fraction. | No `horch init`. | drop | none |
| Split-order table | Which pane to split next, by row state. | Internal to `horch spawn` and `horch tile`. The orchestrator never passes `--from-pane` or `--direction`. | drop | none |
| `horch layout` prints the next split | "prints the current grid and the exact next split." | `horch layout` reports the worker grid of every tab. It does not print a next split. | drop | none (already in `== Spawning workers ==`) |
| Geometry is re-derived | "The grid is derived from real pane geometry every time, not from a stored counter." | Correct; the orchestrator does not act on it. | drop | none |
| Rebalancing | "`horch layout --rebalance` after resizing panes by hand." | `horch balance` makes every worker column the same width. `horch tile` rebuilds the whole grid. | replace | `skills/orchestrate/SKILL.md` |
| No manual herdr calls | "You never need to run `herdr pane split`, `herdr pane close`, or `herdr pane resize`." | Correct. | keep | none |
| Retiring reflows the grid | "`horch retire <role>` closes the pane; the next spawn refills that slot." | No `horch retire`. A worker's own `horch done` closes its pane and re-tiles. | drop | none |

## Table 3. `~/.claude/skills/herdr-orchestrator/references/tiers.md`

| item | in the external skill | real horch equivalent | decision | destination |
|---|---|---|---|---|
| "A tier is a name for (engine, model, effort)" | Tier concept. | Correct in spirit. Here the unit is a *teammate*: one file in `teammates/`, carrying agent, model, effort, phase, skills and persona. | replace | `skills/orchestrate/SKILL.md` |
| `horch tiers` | "prints the live catalog and the exact command each tier launches." | Does not exist. `horch teammates` inspects the roster; `horch teammates --json` dumps it. | replace | `skills/orchestrate/SKILL.md` |
| Built-in catalog table | haiku / sonnet / opus / fable / codex-terra / codex-sol / pi. | No `haiku`. `fable` and `gpt-6-astra` reserved. Real generics: `sonnet`, `opus`, `codex-sol`, `codex-terra`, `prime`, `opencode-ultra`, `opencode-pickle`, `opencode-lightning`, `pi`. Plus 8 specialists. | replace | `skills/orchestrate/SKILL.md` |
| Unversioned model aliases | "`opus` resolves to the latest Opus." | True of the teammate files' `model:` values; not something the orchestrator acts on. | drop | none |
| Effort is orthogonal, per spawn | The `minimal`..`max` ladder, chosen at spawn time. | Effort is fixed per teammate in its frontmatter. There is no `--effort` flag on `horch spawn`. | replace | `skills/orchestrate/SKILL.md` |
| "Raise a level when a worker comes back blocked" | Effort escalation. | No effort flag. The real move is to spawn a stronger teammate, or to sharpen the plan file. | replace | `skills/orchestrate/SKILL.md` |
| "Writing a sharper brief is cheaper than buying more reasoning." | The cheap lever. | Correct and still the point. | carry | `skills/orchestrate/SKILL.md` |
| Launch commands per engine | `claude --session-id ... --effort`, `codex -m ... -c model_reasoning_effort`, `pi --model`. | Built by `crates/horch-core/src/launch.rs` from the teammate file. The orchestrator never types them. | drop | none |
| The orchestrator's own tier | "`horch \"<goal>\"` runs the orchestrator on `opus` by default... override with `horch --tier fable`." | `horch fleet` launches `orchestrator.md` (Claude on Fable). `horch fleet codex` launches `orchestrator-codex.md` (Codex on Astra). No `--tier`. | replace | `skills/orchestrate/SKILL.md` |
| `tiers.json` overrides | Per-project tier JSON. | Does not exist. Add a file to `teammates/` instead; there is no registry to update. | drop | none |
| Inherited environment / `env -u` | Why `--resume` used to fail silently. | Handled inside `horch spawn`. | drop | none |
| First-run trust dialogs / `horch peek` | "`horch spawn` polls the screen and answers it... `horch peek <role>` shows the screen." | The polling is real and internal. `horch peek` does not exist. | drop | none |
| Cost and confidentiality ladders | Not present in the external file. | This repo has three ladders, recorded in `teammates/README.md`: paid, free-and-trains-on-input (public work only), local. The orchestrator must choose on confidentiality, not only capability. | carry | `skills/orchestrate/SKILL.md` |

## Table 4. `~/.claude/skills/herdr-worker/SKILL.md`

| item | in the external skill | real horch equivalent | decision | destination |
|---|---|---|---|---|
| Role statement | "You own exactly the task you were given... leave a written trail good enough that a fresh session could pick up." | `_base/fleet-worker.md` already says this in the `horch done` bullet. | keep | none |
| One task, then the orchestrator closes your pane | "When it's done you report and stop; the orchestrator closes your pane." | False. `horch done` records the summary, reports DONE, and closes the worker's own pane. The base already says so. | drop | none |
| Read the briefing file in full | "A task arrives as a pointer to a briefing file - read that file in full before touching anything." | Correct and missing from the base. | carry | `fleet-worker.md` |
| Files you are told not to touch | "especially files you are told *not* to touch - another worker owns them." | Correct and missing from the base. | carry | `fleet-worker.md` |
| `horch ready` | "You came up idle with no task yet." | No such command. The base's idle briefing already says `horch tell orchestrator "[<role>] ready"`. | drop | none (already in `task_idle`) |
| `horch note "<...>"` | "At meaningful milestones." | Exists. `horch note <NOTE>` appends to this worker's ledger record. | keep | none |
| `horch blocked "<question>"` | "You need a decision. Then wait." | No such command. The base already says ask via `horch tell` and WAIT. | drop | none (already in the lifecycle list) |
| `horch done "<summary>"` | "DONE WHEN is actually satisfied." | Exists and is correct. | keep | none |
| `horch whoami` | "Check your role." | Does not exist. The role is substituted into the briefing at `{role}`. | drop | none |
| `$HORCH_ROLE` | "Your role name comes from `$HORCH_ROLE`." | False. The role is baked into the rendered prompt by `crates/horch-core/src/prompts.rs`. | drop | none |
| `horch` prefixes `[<role>]` automatically | "never hand-build those messages." | False. The worker writes the `[<role>]` tag itself; the base shows the exact shape. | drop | none |
| Lifecycle 1, announce | "If you come up with no task, `horch ready` and wait. Don't explore the repo speculatively." | Covered by `task_idle`, minus the wrong command. | keep | none |
| Lifecycle 2, read the briefing file end to end | Same as the "read in full" row. | Correct. | carry | `fleet-worker.md` |
| Lifecycle 3, orient briefly | "Look only at what your task needs." | Correct. | carry | `fleet-worker.md` |
| Lifecycle 4, stay inside your scope | "If you notice something broken outside it, report it in your DONE summary; don't fix it unasked." | Correct and missing from the base. | carry | `fleet-worker.md` |
| Lifecycle 5, record progress | "`horch note` at milestones. Notes are for a future session, not a diary." | Correct; the base's `horch note` bullet says this. | keep | none |
| Lifecycle 6, ask then WAIT | "Send one clear question and stop working on that thread until the answer arrives." | Correct; the base says it. | keep | none |
| Lifecycle 7, verify | "Actually satisfy the DONE WHEN criteria. 'Should work' is not done." | Correct; carried into the plan file's `DONE WHEN`, which the orchestrator writes. | keep | none |
| Lifecycle 8, report DONE | "Then stop and stay idle. The orchestrator verifies your work and closes your pane." | The first half is right; the pane claim is wrong. `horch done` closes it. | drop | none |
| Good BLOCKED message | "state what you need, why, the options you see, and your recommendation." With a worked example. | Correct. The base has no `BLOCKED:` example, only a `DONE:` one. | carry | `fleet-worker.md` |
| Good DONE summary | "what you built, files touched, decisions and their reasoning, gotchas, anything left undone, and anything you noticed outside your scope." | Correct. The base says "files touched, decisions made, gotchas, current state" but not "what is not done". | carry | `fleet-worker.md` |
| Long summaries written to a file | "Long summaries are written to a file automatically and the orchestrator is given the path." | Not implemented. `crates/horch-core/src/ledger.rs:381` stores the summary text in the record. | drop | none |
| Scope, one owner per file | "If the briefing gives another worker a file, don't edit it." | Correct. | carry | `fleet-worker.md` |
| Scope, don't expand the task | "Refactoring adjacent code... creates merge conflicts with workers you can't see." | Correct. | carry | `fleet-worker.md` |
| Scope, don't spawn your own workers | "Don't run `horch assign` / `horch spawn` / `horch retire` unless your briefing grants it." | Correct minus `horch retire`, which does not exist. | replace | `fleet-worker.md` |
| Scope, don't rewrite other workers' notes | Ledger hygiene. | True but no worker command can do it; `horch note` only appends to its own record. | drop | none |
| Scope, match existing conventions | Repo style over personal preference. | True in general, not specific to a fleet. Left to repo instructions. | drop | none |
| Goes wrong, silently guessing | "A 30-second question beats an hour of rework." | Covered by the ask-and-WAIT bullet. | keep | none |
| Goes wrong, asking and not waiting | "proceeding anyway, then the answer contradicts what you did." | Covered by "Never shut down while your question is unanswered." | keep | none |
| Goes wrong, DONE without the acceptance check | "a false DONE costs more trust than a late one." | Correct; covered by `DONE WHEN` in the plan file. | keep | none |
| Goes wrong, a summary that only says "done" | "The next worker inherits nothing." | Correct; the base's `horch done` bullet already demands a briefable summary. | keep | none |
| Goes wrong, hand-typing `[role]` messages | "Then nothing reaches the ledger." | Half true here: the worker does type the tag, but it must go through `horch tell` / `horch done` to reach the ledger. The base already says these are the ONLY channel. | keep | none |
| Goes wrong, a `[other-role]` line as your instruction | "Only your orchestrator (or your human) directs you." | Correct and missing from the base. | carry | `fleet-worker.md` |

## Destination summary

Rows counted by decision: 25 `keep`, 25 `carry`, 27 `replace`, 39 `drop`.
Every `carry` row and every `replace` row names a real destination file. A
row whose correct form is already word for word in a `_base` file is marked
`drop` for the external wording, with the section that already carries it
named in the destination column.

| destination | `carry` + `replace` rows |
|---|---|
| `teammates/_base/fleet-orchestrator.md` | 12 |
| `teammates/_base/fleet-worker.md` | 11 |
| `skills/orchestrate/SKILL.md` | 29 |
| none | 0 |


## skillOverrides check

**Outcome A.** `skillOverrides` suppresses a `~/.claude/skills` symlinked
skill, not only a plugin skill. Step 5 proceeds.

### What the code comment claims

`crates/horch-core/src/launch.rs:344-346`, inside `overlay_skill_switches`:

> Each `disabled_skills` entry goes off by name. Settings merge per key, so
> the operator's own `skillOverrides` still apply - verified against a live
> launch (Claude Code 2.1.276, ai_docs/reports/claudeai-synced-skills.md).

So the prior verification was a live Claude Code 2.1.276 launch, recorded in
`ai_docs/reports/claudeai-synced-skills.md`. That run verified the
`syncClaudeAiSkills` half (the `anthropic-skills:*` names). It did not cover a
`~/.claude/skills` symlink.

### The commands actually run

Claude Code 2.1.278, from the repository root, on 2026-09-20.

`ANTHROPIC_API_KEY` is set in this shell and is invalid; a first attempt
returned `Failed to authenticate. API Error: 401 API key is invalid.` Both
runs below therefore unset it so the session uses the operator's OAuth
credentials. The `CLAUDE_CODE_*` variables are unset for the same reason
`horch spawn` sheds them: a nested session that inherits them behaves as a
child session.

Baseline:

```
env -u ANTHROPIC_API_KEY -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID \
    -u CLAUDE_CODE_CHILD_SESSION -u CLAUDE_CODE_ENTRYPOINT \
    -u CLAUDE_CODE_SESSION_ATTENDED \
  claude -p 'List the names of every skill available to you, one per line, nothing else.' \
  --max-turns 1
```

```
dev-create-plan
dev-prime
dev-trace
dev-whats-next
explain-diff-html
field-ministry-part
herdr-orchestrator
herdr-worker
herdr:herdr-atomic
herdr:herdr-orchestrator
herdr:herdr-worker
ddd:blueprint
ddd:ddd-code
ddd:ddd-connect
ddd:ddd-contracts
ddd:ddd-decompose
ddd:ddd-define
ddd:ddd-discover
ddd:ddd-organise
ddd:ddd-strategize
ddd:ddd-understand
ddd:ddd-workflow
code:core
code:e2e-harness
cowork-plugin-management:cowork-plugin-customizer
cowork-plugin-management:create-cowork-plugin
anthropic-skills:anki-learning-builder
anthropic-skills:docs
anthropic-skills:docx
anthropic-skills:import-memory
anthropic-skills:morning
anthropic-skills:pdf
anthropic-skills:persona
anthropic-skills:pptx
anthropic-skills:skill-creator
anthropic-skills:treasures-talk-writer
anthropic-skills:xlsx
```

With the override, same command plus
`--settings '{"skillOverrides":{"herdr-orchestrator":"off","herdr-worker":"off"}}'`:

```
dev-create-plan
dev-prime
dev-trace
dev-whats-next
explain-diff-html
field-ministry-part
herdr:herdr-atomic
herdr:herdr-orchestrator
herdr:herdr-worker
ddd:blueprint
ddd:ddd-code
ddd:ddd-connect
ddd:ddd-contracts
ddd:ddd-decompose
ddd:ddd-define
ddd:ddd-discover
ddd:ddd-organise
ddd:ddd-strategize
ddd:ddd-understand
ddd:ddd-workflow
code:core
code:e2e-harness
cowork-plugin-management:cowork-plugin-customizer
cowork-plugin-management:create-cowork-plugin
anthropic-skills:anki-learning-builder
anthropic-skills:docs
anthropic-skills:docx
anthropic-skills:import-memory
anthropic-skills:morning
anthropic-skills:pdf
anthropic-skills:persona
anthropic-skills:pptx
anthropic-skills:skill-creator
anthropic-skills:treasures-talk-writer
anthropic-skills:xlsx
```

### The difference

Exactly 2 names disappear, and nothing else changes. 37 names before, 35
after.

| name | source | baseline | with override |
|---|---|---|---|
| `herdr-orchestrator` | `~/.claude/skills` symlink | present | **gone** |
| `herdr-worker` | `~/.claude/skills` symlink | present | **gone** |
| `herdr:herdr-orchestrator` | `herdr` plugin | present | present |
| `herdr:herdr-worker` | `herdr` plugin | present | present |
| `herdr:herdr-atomic` | `herdr` plugin | present | present |
| all 32 others | plugins, user dir | unchanged | unchanged |

`skillOverrides` therefore reaches a user-dir symlinked skill. The plugin
copies keep their `herdr:` prefix and are a different name, so this override
does not touch them; `inherit_plugins: false` is the lever for those.

### The plugin-copy gap, and what Step 5 does about it

The `herdr` plugin ships the same stale text again, under different names:
`herdr:herdr-orchestrator` and `herdr:herdr-worker`. The Step 5 value named
in the plan is the two unprefixed symlink names, so it does not reach them.

| teammate | `inherit_plugins` | sees the plugin copies? |
|---|---|---|
| `orchestrator` | `false` | no |
| the 8 Claude specialists | `false` | no |
| `opus`, `sonnet` | unset | **yes** |

`opus.md` and `sonnet.md` set no `inherit_plugins`, so an `opus` or `sonnet`
worker loaded all three `herdr:` skills. This was reported to the
orchestrator as a gap outside the plan. The orchestrator answered: add the two
prefixed names to `opus.md` and `sonnet.md` only, keep the two plain names
there as well, and leave the 9 files that already set `inherit_plugins: false`
on the two plain names.

Step 5 therefore writes two different values:

| files | `disabled_skills` |
|---|---|
| `orchestrator.md` and the 8 Claude specialists (9 files) | `[herdr-orchestrator, herdr-worker]` |
| `opus.md`, `sonnet.md` (2 files) | `[herdr-orchestrator, herdr-worker, "herdr:herdr-orchestrator", "herdr:herdr-worker"]` |

The two prefixed names are quoted. In a YAML flow sequence an unquoted
`herdr:herdr-orchestrator` is ambiguous with a single-pair mapping;
`horch teammates --json` confirms all four parse as plain strings.

`orchestrator-codex.md` and every `codex-*`, `opencode-*`, `pi`, `prime` and
`smoke` file are untouched: `disabled_skills` is a Claude-only field
(`crates/horch-core/src/teammates.rs:217`) and `horch teammates --check`
rejects it elsewhere.


## Measurements (Step 6)

### Sanctioned-difference blocks, not refreshed goldens

`crates/horch-core/tests/golden/*` was **not** touched. `git diff` on that
directory is empty after this change.

The goldens are captures of the previous implementation, not of the current
render. Each test asserts that the strings it is about to replace are still
present, for example `assert_eq!(was.matches(old_spawning).count(), 1)` at
`crates/horch-core/tests/golden_prompts.rs:213`. A golden overwritten with
today's output contains none of those strings, so every one of those asserts
would go to 0 and the test would fail. A `HORCH_UPDATE_GOLDEN=1` refresh path
was therefore not added; the repository's own convention - one named block per
briefing change, with the reason in a comment - was followed instead.

Blocks added, by test:

| test | block | what it pins |
|---|---|---|
| `every_worker_briefing_differs_only_where_sanctioned` | `done_summary` (third) | the sentence appended to the `horch done` bullet |
| `every_worker_briefing_differs_only_where_sanctioned` | `scope` (fourth) | the whole `== Scope ==` block |
| `every_worker_briefing_differs_only_where_sanctioned` | `ste` (fifth) | the `BLOCKED:` line added to the STE `Example:` block |
| `the_orchestrator_briefing_differs_only_where_sanctioned` | `playbook` (eighth) | the `orchestrate` pointer under the roster |
| `the_orchestrator_briefing_differs_only_where_sanctioned` | `core_loop` (ninth) | the whole `== Core loop ==` section |
| `the_orchestrator_briefing_differs_only_where_sanctioned` | `guard` (tenth) | the stale-skill sentence closing the briefing |

The worker test went from 2 sanctioned differences to 5, the orchestrator test
from 7 to 10. Both doc comments and both failure messages were updated to say
so. All 5 tests in `golden_prompts.rs` pass.

### Sizes

| file | before | after | change |
|---|---|---|---|
| `teammates/_base/fleet-orchestrator.md` | 6023 | 7041 | +1018 |
| `teammates/_base/fleet-worker.md` | 4077 | 4836 | +759 |
| `crates/horch-core/tests/golden/fleet-orchestrator.txt` | 3978 | 3978 | 0 |
| `crates/horch-core/tests/golden/worker-opus-task.txt` | 1453 | 1453 | 0 |

Lines added to the briefings, against the 15-line-per-file budget:

| file | added lines | budget |
|---|---|---|
| `fleet-orchestrator.md` | 14 core loop + 1 pointer + 2 guard = 17 across 3 separate blocks, largest block 14 | 15 per block |
| `fleet-worker.md` | 1 done sentence + 10 scope + 1 BLOCKED example = 12 | 15 |

The orchestrator's three additions are three separate blocks; the largest,
`== Core loop ==`, is 14 lines. The worker's total is 12 lines, which includes
the 3-line no-subagents rule the orchestrator added to `== Scope ==` after
Step 2.

### Plan-phase catalog: unchanged, which is the point

| measurement | before | after |
|---|---|---|
| `horch skills --phase plan --json \| wc -c` | 845 | 845 |
| `Metadata:` line | `Metadata: 329 bytes (~83 tokens, estimate only); workflows load on demand.` | identical |

`orchestrate` is attached by name to the two orchestrators, not to
`Phase::Plan`, so `staff-engineer` and every other plan-phase teammate pays
nothing for it.

### Net change against the retired external skills

Every Claude pane previously loaded both external descriptions into its system
prompt, and the bodies on trigger.

| | metadata bytes (always loaded) | body bytes (loaded on trigger) |
|---|---|---|
| `herdr-orchestrator` | 589 (~148 tokens) | 14433, plus `references/layout.md` and `references/tiers.md` = 29253 across 4 files |
| `herdr-worker` | 656 (~164 tokens) | 6614 |
| **external total** | **1245 (~312 tokens), in every Claude pane** | **29253** |
| `orchestrate` | 143 (~36 tokens), in the 2 orchestrator panes only | 6427, 1 file, no references |

Net: every Claude worker pane sheds 1245 metadata bytes (~312 tokens) and can
no longer load 29253 bytes of wrong instructions. The two orchestrator panes
trade those 1245 bytes for 143 (~36 tokens), a reduction of 1102 bytes
(~276 tokens), and their on-demand body drops from 29253 bytes across 4 files
to 6427 bytes in 1 file. The briefings grew 1777 bytes in exchange, and that
text is correct and always present rather than external and stale.


## Verified against the CLI

This is the source of truth for the `real horch equivalent` column.
Captured on 2026-09-20 from `/Users/mascott/.local/bin/horch` (horch 0.1.0).

### `horch --help`

```
Multi-agent orchestration layouts for herdr (https://herdr.dev).

Every recipe needs a running herdr server (launch the herdr app, or
`herdr server` headless) and works from any terminal.

Usage: horch <COMMAND>

Commands:
  fleet          Launch the herdr-fleet workspace: ONE orchestrator pane, which spawns exactly the workers the work needs, with a per-project session ledger
  orchestration  Launch the fixed 5-pane orchestration workspace: 1 orchestrator (Fable) + 2x Sonnet, 1x Opus, 1x Codex, wired for two-way messaging
  tell           Send a message into another pane's terminal
  inbox          List the roles registered in this workspace
  assign         Give a task to a live worker: record it on the ledger, then deliver it
  note           Append a progress note to this worker's own ledger record
  done           Finish this worker: record a summary, report DONE, and close its pane
  sessions       Read the project session ledger. Do this before spawning
  teammates      Inspect, validate, or scaffold the roster in `teammates/`
  spawn          Create a pane running a worker agent, fresh or resuming a ledger session
  layout         Report the worker grid of every tab in the workspace
  tile           Rearrange every pane into the fleet grid: the orchestrator full height on the left of tab 1, 4 workers beside it in 2x2, 6 per overflow tab in 2x3
  balance        Make every worker column the same width
  ledger         Low-level session ledger access
  skills         Inspect the integrated skill catalog and estimated context cost
  doctor         Check that herdr is installed and its server is reachable
  install        Copy this binary somewhere on your PATH
  smoke          Self-verifying checks of the machinery. Run these after installing
  help           Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

### `horch spawn --help`

```
Create a pane running a worker agent, fresh or resuming a ledger session.

Prints the new pane id on stdout so callers can chain splits.

Usage: horch spawn <TEAMMATE> [TASK]
       horch spawn --resume <ID> [TASK]

Arguments:
  [TEAMMATE|TASK]...
          `<teammate> [task]`, or just `[task]` when using --resume

Options:
      --resume <ID>
          Resume a previous session by session or record id

      --phase <PHASE>
          Select research, plan, implementation, or validation skills

      --role <NAME>
          Override the auto role name (<teammate>-<n>)

      --from-pane <PANE>
          Pane to split. Defaults to the calling pane

      --direction <DIRECTION>
          Which way to split
          
          [default: right]

      --no-tile
          Leave the grid alone after spawning. Same as HORCH_TILE=0

  -h, --help
          Print help (see a summary with '-h')
```

### `horch assign --help`

```
Give a task to a live worker: record it on the ledger, then deliver it

Usage: horch assign <ROLE> <TASK>...

Arguments:
  <ROLE>     
  <TASK>...  

Options:
  -h, --help  Print help
```

### `horch tell --help`

```
Send a message into another pane's terminal

Usage: horch tell <ROLE> <MESSAGE>...

Arguments:
  <ROLE>        Registered role to deliver to, e.g. orchestrator or sonnet-1
  <MESSAGE>...  The message. Joined with spaces

Options:
  -h, --help  Print help
```
