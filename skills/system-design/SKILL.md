---
name: system-design
description: Use before writing an implementation plan for a change that crosses components, adds a service or store, changes a data shape, or is hard to undo - frames constraints, compares real options with their tradeoffs, maps data flow and failure modes, sets the migration and rollout order, sizes the blast radius, and records the decision so a planner and a junior builder can work from it.
---

# System design

Decide how a thing should be built, and write the reasoning down so that someone can build it, review it, and safely deviate from it. This skill produces the design. `create-plan` then turns the design into numbered, verifiable steps. For a small local change, skip this skill and go straight to `create-plan`.

## When to use

Use it when one of these is true:

- The change touches two or more components, services, processes or repositories.
- It adds a store, a queue, a cache, a background job, an external dependency or a public interface.
- It changes the shape of persisted data or of a message on the wire.
- It is expensive to undo: a data migration, a public API, a file format, a protocol.
- Two senior people would reasonably build it in different ways.

## Inputs

- The requirements (acceptance criteria, scope, non-goals). If there are none, write a five-line version of them yourself, mark it `[assumed]`, and send it to the orchestrator in a `QUESTION:` while you continue.
- The current system: read the code on the request path end to end, the persistence layer, the deployment and config files, and the tests that pin today's behaviour. Follow one real request across every component it touches, from entry point to storage and back (use `trace` if your briefing lists it).
- Existing design records (`docs/`, `ai_docs/`, ADRs). A new design that contradicts an old decision says so and says why.

## Workflow

1. **State the problem and the forces.** Write the goal in two or three sentences. Then list the constraints that shape the answer, each with its source:
   - functional: what must be true afterwards (from the requirements);
   - quality: latency, throughput, data size, availability, consistency, cost, with numbers where you can find them;
   - context: the languages, runtimes, stores and deploy model already in use; team and operations limits; compatibility promises.
   Check: each constraint has a source (a file, a requirement ID, a measurement) or is marked `[assumed]`.

2. **Map the current system.** Draw the relevant components and the data flow as a short list or an ASCII diagram: who calls whom, synchronously or not, what each stores, where the trust boundaries are. Note where the change lands. Check: every arrow is something you saw in the code or config, with a path.

3. **Generate real options.** Write at least two options that could both work, and three when the decision is expensive to undo. "Do nothing" or "the smallest change" is often one of them. A straw man that nobody would pick is not an option. For each option give: the shape in a few lines, what it changes, and what it costs to build.

4. **Compare the options against the forces.** Use one table, with the constraints from step 1 as rows. Add these rows every time:
   - **Failure modes:** what breaks, and what the user sees, when each new dependency is slow, down or wrong.
   - **Consistency:** what can be stale or duplicated, for how long, and who notices.
   - **Operability:** how an operator sees it working (logs, metrics), and how they fix it at 3 a.m.
   - **Reversibility:** what undoing it in six months costs.
   - **Blast radius:** what else must change, and what else could break, if this ships with a bug.
   Check: every cell is a concrete statement, not "good" or "bad".

5. **Choose, and say why.** Pick one option. In one paragraph, name the forces that decided it. Then name each rejected option with the specific reason it lost. A builder who hits a surprise reads this to know which deviations are safe.

6. **Specify the interfaces and data.** For the chosen option, write the contracts that other code depends on: function or endpoint signatures, message and record shapes, error cases, ordering and idempotency promises, ownership of each piece of data. Give the interface itself, not a description of it. For an HTTP or RPC boundary, `api-contracts` has the checklist; for persisted data, `data-migrations` has the change order.

7. **Walk the failure modes.** For each new call, store or job: what happens on timeout, on a partial write, on a duplicate delivery, on a restart halfway through, on bad input, on overload. Each one either recovers by a stated mechanism or fails in a stated, visible way. Check: no failure path ends in "should not happen".

8. **Order the rollout.** Write the order in which the parts ship so that each step is safe to deploy alone and safe to roll back:
   - additive changes first (new tables, new fields, new endpoints, readers that accept both shapes);
   - then writers; then the switch of reads; then removal of the old path;
   - a feature flag or config switch where a step changes behaviour for users;
   - the check that proves each step worked before the next step starts.
   Check: at every point in the order, old and new code running side by side is correct.

9. **Name the non-goals and the deferred work.** What this design deliberately does not do, and what a later change will need. This stops scope from growing during the build.

10. **Write the design and report.** Write it to the path in your task, or to `ai_docs/designs/YYYY-MM-DD-<topic>.md`, with the template below. Hand it to `create-plan` (yourself or the planner the orchestrator names). Send `DONE:` with the path, the chosen option in one line, and the open decisions.

## Output template

```markdown
# <Topic>: design

## Problem and forces
<goal>
| Constraint | Kind | Source |
|---|---|---|

## Current system
<components, data flow, trust boundaries; paths>

## Options
### A. <name>
### B. <name>

## Comparison
| Force | A | B |
|---|---|---|
| Failure modes | | |
| Consistency | | |
| Operability | | |
| Reversibility | | |
| Blast radius | | |

## Decision
<chosen option and the forces that decided it>
Rejected: B because ...

## Interfaces and data
<signatures, shapes, errors, ordering, idempotency, data ownership>

## Failure modes
| Event | Behaviour | Visible as |
|---|---|---|

## Rollout order
1. <step> - safe alone because ... - proven by ...

## Non-goals and deferred work

## Open decisions
| Decision | Recommendation | Owner |
|---|---|---|
```

## Heuristics

- Prefer the boring option that the team already runs, unless a force from step 1 rules it out. A new store or runtime is an operations cost for every later change.
- Put each piece of knowledge in one place. If two components must agree on a rule, one owns it and the other asks.
- Make the expensive-to-undo parts small and the cheap-to-undo parts carry the uncertainty.
- Design the data first. Code is easier to change than persisted data with live readers.
- Synchronous calls couple availability: if A calls B inline, A is down when B is down. Say whether that is acceptable.
- A retry without idempotency is a duplicate. A queue without a dead-letter path is a silent loss.
- When a number decides between options and you do not have it, say what to measure and which answer picks which option. Do not guess it.

## Review checklist

- [ ] Each constraint has a source or an `[assumed]` mark.
- [ ] At least two real options; the comparison covers failure, consistency, operability, reversibility and blast radius.
- [ ] Rejected options have specific reasons.
- [ ] Interfaces are written out, with errors, ordering and idempotency.
- [ ] Every new dependency has a stated behaviour when slow, down or wrong.
- [ ] Each rollout step is safe alone and has a check; old and new code can run side by side.
- [ ] Non-goals are explicit.
