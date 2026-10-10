---
name: context-messages
description: >
  Every line horch types for the context watch; Rust only fills the
  placeholders. Each message is 1 line: a newline would submit early. The
  briefings in fleet-worker.md and fleet-orchestrator.md name these
  prefixes; change both together. Operators do not need to copy this file.
messages:
  request: "NOTE: Your context is {tokens} tokens. Your threshold is {threshold} tokens. At your next stopping point, write {handoff} with the horch:handoff skill. Then run: horch note \"handoff: {handoff}\". Then send: horch tell orchestrator \"[{role}] NOTE: COMPACT-READY {handoff}\". Then stop and wait."
  warning: "NOTE: Context warning. Your context is {tokens} tokens. Your threshold is {threshold} tokens. This horch note call is your stopping point. Do not start new work. Write {handoff} with the horch:handoff skill. Then run: horch note \"handoff: {handoff}\". Then send: horch tell orchestrator \"[{role}] NOTE: COMPACT-READY {handoff}\". Then end your turn and wait."
  warning-orchestrator: "NOTE: Context warning. Your context is {tokens} tokens. Your threshold is {threshold} tokens. At your next stopping point, compact yourself as == Context watch == in your briefing says. Your handoff file is {handoff}."
  instructions: "Keep in the summary: your role {role}, your plan or brief path, your handoff file {handoff}, the files you touched and their state, your decisions, the open questions you sent and their answers, and your report target. The full state is in {handoff}."
  instructions-orchestrator: "Keep in the summary: the roster of live roles with teammate and pane, the map of which role owns which plan file, your plan file, every open question and its answer, every COMPACT-READY line you did not yet act on, and your decisions. The full state is in {handoff}."
  resume: "NOTE: Compaction is complete. Read {handoff} now. Then continue your task from its next step. Do not repeat finished steps."
  reported: "[horch] NOTE: {role} compacted. Context {pre} -> {post} tokens. Handoff: {handoff}."
  failed: "[horch] BLOCKED: Compaction of {role} failed at step {step}: {reason}. Log: {log}."
---
# Context messages

horch types these lines into a pane for the context watch.

| key | who gets it | when |
|---|---|---|
| `request` | a worker | the orchestrator asks it to prepare for compaction |
| `warning` | a worker | its `horch note` call finds it at or over its threshold |
| `warning-orchestrator` | the orchestrator | its own context is at or over its threshold |
| `instructions` | a worker | the argument of `/compact`, where the harness takes one |
| `instructions-orchestrator` | the orchestrator | the argument of its own `/compact` |
| `resume` | the compacted session | after horch confirms the compaction |
| `reported` | the orchestrator | after a compaction succeeds |
| `failed` | the orchestrator | after a compaction fails |

Placeholders: `{role}`, `{tokens}`, `{threshold}`, `{handoff}`, `{pre}`,
`{post}`, `{step}`, `{reason}`, `{log}`. `horch teammates --check` fails any
other placeholder, a missing key, an empty message, and a message with a
newline. horch replaces a newline in a placeholder value with a space, so
each line stays 1 line.

## Prefixes the briefings name

| prefix in a briefing | message key(s) it starts or contains |
|---|---|
| `NOTE: Your context is` | starts `request` |
| `NOTE: Context warning` | starts `warning`, `warning-orchestrator` |
| `NOTE: COMPACT-READY` | `request` and `warning` contain `[{role}] NOTE: COMPACT-READY {handoff}` |
| `[horch] NOTE:` | starts `reported` |
| `[horch] BLOCKED:` | starts `failed` |

An operator does not need to copy this file; a change here lands with the matching briefing change.
`horch teammates --check` warns when a copy differs from the built-in file.
