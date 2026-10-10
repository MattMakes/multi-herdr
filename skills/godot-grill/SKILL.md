---
name: godot-grill
description: 'Use when a new Godot system or feature has open design decisions — writes the decision record with a proposed answer for each, scope first, then sends the orchestrator one question that lists them, before any design or code. Triggers on "what do I need to decide before building", "open design decisions", "settle the scope first". Not for choosing between nodes or APIs, and not for bug fixes.'
---

# Godot Grill

Settle the decisions only the operator can make, before anyone designs a scene tree or writes
code. In a fleet you have no human in your pane: the orchestrator answers for the operator. The
output is a **decision record**, not a design.

> **Related skills:** **godot-brainstorming** for the scene tree, signal map, and plan once decisions are settled, **godot-scene-organization** for composition vs. inheritance trade-offs.

## 1. Decisions, not facts

Ask only what **only the operator knows**: intent, constraints, priorities, taste.

Node types, API signatures, and version differences are **facts**. Look them up
(`godot-brainstorming/references/node-selection.md`, the domain skills), decide, and record the
choice. Never spend a question on one.

Architecture choices — data home, state representation, save format — are decided **from** the
operator's answers, not asked as technology picks. Ask the constraint behind them ("will designers
edit items in the Inspector?"), then map the answer to Resource `.tres` yourself.

| Question | Verdict |
|---|---|
| "Throwaway prototype, or a system other code builds on?" | Decision — ask |
| "Should the player be a `CharacterBody2D` or a `RigidBody2D`?" | Fact — decide, record it |
| "When two clients disagree, who is right?" | Decision — ask |
| "Can a `Tween` chain steps in 4.3?" | Fact — never ask |

## 2. The seeded dependency tree

Four roots have no prerequisites:

| Root | Options |
|---|---|
| **Scope** | throwaway slice / one feature / a system others build on |
| **Dimension** | 2D / 3D / 2.5D |
| **Language** | GDScript / C# / both |
| **Authority** | single-player / networked (and if networked, who is authoritative) |

| Settling this… | …unblocks |
|---|---|
| Scope | prunes branches: a throwaway slice skips persistence, data home, networking, and testing |
| Authority | state ownership (source of truth); signals vs. RPCs |
| Dimension | physics model; camera model |
| Language | interop boundary, when the answer is "both" |
| Scope + Dimension | entity model: composition vs. inheritance |
| Entity model | data home (Resource `.tres` / autoload / node-local `@export`); communication (signals up, calls down / EventBus / DI) |
| Authority + Entity model | state representation (enum FSM / node FSM / AnimationTree / none); persistence boundary |
| Data home + Persistence | save format (ConfigFile / JSON / Resource serialization) |

The right-hand column names what an answer lets **you** decide; ask the orchestrator the constraint
behind it, never the technology. The tree is a **seed, not a script**. Answers grow it — "networked" creates branches a
single-player answer never does. Skip any root the request or the project already answers
(`project.godot`, existing scripts). Most sessions visit few nodes.

## 3. Fill the record, then ask once

**Before you write anything**, read where the project keeps decision records, checking in order:
your plan file and the project's agent instructions file (`CLAUDE.md`, `AGENTS.md`, …); then an
existing decisions or ADR directory. A recorded decision is a settled prerequisite: never re-ask
it; start the frontier past it.

The **frontier** is every open decision whose prerequisites are settled. Walk the seeded tree
(§2) and take scope first — scope prunes the most tree. A "throwaway slice" answer commonly
leaves only a few decisions.

Write the decision record (§5) now. For each open decision, fill the row with your
recommended answer and mark it `[proposed]`:

```text
| Bag model | Fixed 20 slots `[proposed]` | Grid UI already designed | a weight system is wanted |
```

A `[proposed]` answer is valid until the orchestrator overrides it. Proceed on it.

Then send **one** `QUESTION:` to the orchestrator that lists all open decisions, numbered, scope
first, each with its recommended answer:

```text
horch tell orchestrator "[<role>] QUESTION: Open decisions for <topic>. Record: <path>.
Q1 Scope: <the question, with its options>. Proposed: <answer, and why in one clause>.
Q2 ..."
```

Send one message, not one per decision. A decision whose prerequisite is still open goes in the
same message with its own proposed answer, marked as depending on the earlier one.

When the orchestrator answers, replace each overridden `[proposed]` with the answer. If an
answer contradicts a recorded decision, say so in the reply and ask whether to reopen it — never
overwrite a record silently.

## 4. Ending the grill

The grill ends when the record has no open row without a `[proposed]` or settled answer and the
orchestrator has had your one `QUESTION:`. Do not wait for a reply before you start work that
depends only on settled or `[proposed]` rows. Then hand off:

- it needs a scene tree, signal map, or plan → `godot-brainstorming`, from Step 2
- it is a single known change → the matching domain skill

## 5. The decision record

This skill has no decisions folder of its own. Write the record where this project keeps
them, checking in order: your plan file and the project's agent instructions file (`CLAUDE.md`,
`AGENTS.md`, …); then an existing decisions or ADR directory. If none applies, use
`docs/decisions/`. Name the file `YYYY-MM-DD-<topic>.md`. The path must stay stable: the
next grill reads it back.

```markdown
# <Topic> — decisions

| Decision | Choice | Why | Revisit when |
|---|---|---|---|
| Bag model | Fixed 20 slots `[proposed]` | Grid UI already designed | a weight system is wanted |
| Item data | Resource `.tres` | Inspector editing, typed exports | items exceed ~200 |

## Open / deferred
- Equipment stat aggregation — deferred to a later pass.
- Save slots — `[proposed]` 1; revisit before shipping.
```

`Revisit when` keeps a decision reopenable, not binding. A row without `[proposed]` is settled
(by a recorded decision or an orchestrator answer). If you cannot write files, put the record in
your `horch done` summary instead.

## 6. Anti-patterns

| Anti-pattern | Why it is wrong | Instead |
|---|---|---|
| Several `QUESTION:` messages | The orchestrator answers in a vacuum; each one costs a turn | Send one message with the whole list |
| Asking a fact | Spends the orchestrator's attention on your job | Look it up, decide, record |
| A question with no recommendation | The orchestrator must invent the answer | Every question gets a proposed answer |
| Blocking on the reply | The fleet idles | Work on settled and `[proposed]` rows |
| Grilling a bug fix or an explicit ask | The over-correction this skill must not become | Route to the domain skill |
| Re-asking a recorded decision | Wastes the record | Read where the project keeps decision records first |
| Overwriting a settled row | Loses the earlier reason | Ask whether to reopen it |

## Checklist

- [ ] Read existing decision records before you write
- [ ] Scope is the first row and the first question
- [ ] Every open row has a `[proposed]` answer
- [ ] Every question is a decision, numbered, with a proposed answer
- [ ] Sent one `QUESTION:`, not several
- [ ] Record written (or in the `horch done` summary when files cannot be written)
- [ ] Orchestrator answers merged into the record
