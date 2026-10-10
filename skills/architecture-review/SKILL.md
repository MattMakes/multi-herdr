---
name: architecture-review
description: Use when judging whether a change or a design fits the system it lands in - boundaries and dependency direction, honest contracts, new coupling, data ownership, failure isolation and reversibility - ranked by consequence, with file-and-line evidence and the concrete failure each finding permits. Read-only; the implementer decides the fix.
---

# Architecture review

Judge whether a change fits the system. A line-level review (`code-review`) asks "is this code correct?". This review asks "is this the right shape, in the right place, and what will it cost later?". You report. You do not edit. The implementer decides what to do.

## When to use

- A diff, a branch or a design document needs a structural verdict before merge or before build.
- A plan exists, and you must check the implementation against it.
- Not for: style, naming, or local bugs (`code-review`); metrics and complexity numbers (`code-analysis`); security (`security-review`). Hand those findings to the right skill or mention them in one line, at most.

## Inputs

- The change: `git diff <base>...HEAD`, the PR, or the design file.
- The plan or design it claims to follow, if any. A deviation from the plan is a finding only when it matters; say whether it is better or worse than the plan.
- The surrounding system: the modules the change touches, their callers, their tests, and any architecture notes (`docs/`, ADRs, `CLAUDE.md`, a crate or package layout rule, an architecture test such as an import scan).

## Workflow

1. **Learn the intended structure first.** Before you read the diff, write down in three to six lines how the affected part of the system is meant to be layered: which modules may depend on which, who owns which data, where the I/O and the trust boundaries are. Take it from architecture docs and tests if they exist; otherwise infer it from the import graph and mark it `[inferred]`. Check: each line cites a path.

2. **Map what the change does to the structure.** List the new and changed dependencies (imports, calls, shared types, shared tables, events, config keys) as `A -> B` edges. Mark each edge new, removed or changed. Use the language's own tools where they exist (`cargo tree`, `go list -deps`, `madge`, `pydeps`, an IDE call graph), or grep the imports. Check: you can say which edges did not exist before.

3. **Examine the four questions, in this order.**

   **a. Boundaries.** Does knowledge sit where it belongs?
   - A dependency that points the wrong way: domain code that imports the web framework, the database driver or the CLI; a lower layer that calls a higher one; a shared library that knows one caller.
   - A detail that leaks across a seam: a storage type, an ORM entity, an HTTP status or a vendor error in a domain signature; a caller that must know the call order inside another module.
   - A rule that now lives in two places and must stay in sync.
   - Data with two writers, or a component that reads another component's private store.

   **b. Contracts.** Are the interfaces honest?
   - The signature hides a failure that callers must handle (a panic, a sentinel value, an untyped error, an exception that is not declared).
   - Ordering, idempotency, retry and concurrency promises are not stated, or are stated and not kept.
   - A public type or endpoint changed shape with no versioning or compatibility path.
   - Nullable, optional and empty mean different things in different places.

   **c. Coupling.** What else must change when this changes?
   - Temporal coupling: two things that must deploy together, or a synchronous call that makes one service's availability depend on another's.
   - Shared mutable state, global singletons, or a config key read in many places.
   - A wide interface where callers use one method, or a "god" module that the change made bigger.
   - Was the coupling already there? A finding about old coupling is out of scope unless the change makes it worse; say "pre-existing" if you mention it.

   **d. Reversibility and failure isolation.**
   - What does undoing this cost in six months? Persisted formats, public APIs, schema changes and new infrastructure are expensive; internal code is cheap.
   - When the new dependency is slow, down or wrong, how far does the failure spread? Is there a timeout, a bound, a fallback?
   - Does the change add a path that a test cannot reach, or a component that cannot run in tests without real infrastructure?

4. **Prove each finding.** For each candidate finding, find the concrete failure it permits: the future change that becomes hard, the bug that becomes possible, the outage that spreads. Point to file and line. If you cannot name a concrete failure, drop the finding. Check: every finding has a "this permits..." sentence that a builder could turn into a test or an example.

5. **Rank by consequence.** Order findings by what they cost if left: a boundary that forces a rewrite outranks ten naming notes. Use three levels:
   - **Blocker:** wrong in a way that is expensive to undo after merge (persisted format, public contract, dependency inversion that others will build on), or that permits data loss or a spreading outage.
   - **Should fix:** a real structural cost, but cheap to change later.
   - **Consider:** a judgement call; say which way you lean and why.
   Do not manufacture findings. If the design is sound, say so in one sentence and stop. Empty reviews are allowed and useful.

6. **Write the report.** Use the format below. Write it to the path in your task, or return it in `DONE:` if it is short. You hold no Edit or Write permission for code; if you need a file, the task names it.

## Finding format

```
[Blocker] Boundary: domain service imports the HTTP client
  Where:   src/billing/invoice.rs:14, src/billing/invoice.rs:88-102
  What:    Invoice::finalize calls reqwest directly to notify the ledger.
  Permits: every billing unit test needs a live ledger or a mock HTTP server;
           a second notifier (queue) means editing domain code.
  Instead: take a `LedgerNotifier` trait in the constructor; the HTTP
           implementation lives in src/adapters/.
  Evidence: checked in code (imports, call site); plan step 4 put it in adapters.
```

Rules for findings:
- One structural problem per finding. Group repeated instances under one finding with all locations.
- "Instead" is a direction, not a patch. The implementer decides the code.
- Mark each claim as **checked** (you read the code or ran the tool) or **inferred**.
- No style, naming or formatting findings unless a name lies about a contract.

## Report template

```markdown
# Architecture review: <change>

Verdict: <sound | sound with fixes | needs rework> - <one sentence>

## Intended structure
<3-6 lines with paths>

## Dependency changes
<new / removed / changed edges>

## Findings
<findings, ranked>

## Plan conformance
<matches | deviates at ..., better/worse because ...>

## Out of scope, noted once
<one line each, routed to code-review / security-review / code-analysis>
```

## Review checklist

- [ ] I wrote the intended structure before reading the diff, with paths.
- [ ] I listed the new dependency edges.
- [ ] Every finding has a location, a concrete "permits", and checked/inferred marks.
- [ ] Findings are ranked by consequence; nothing is manufactured.
- [ ] Pre-existing problems are labelled as such and not counted against the change.
- [ ] I edited no code.
