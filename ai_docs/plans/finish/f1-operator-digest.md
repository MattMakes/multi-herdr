# F1 operator-digest: a worker runs exactly the skill bytes the ledger recorded

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Owner: opus-56 (wrote O1).

## GOAL

The skill content a worker loads is the content whose digest the ledger
recorded at spawn, or the launch fails with a clear error. Covers operator
skills first; apply the same mechanism to marketplace skills if they share
the gap.

## CONTEXT

- codex-reviewer-2, MEDIUM: `crates/horch/src/cmd/spawn.rs:132-140` reads the
  operator skill into the planning catalog; `execution/plan.rs:351-370`
  records its digest; `execution/service.rs:109-115` stores the refs before
  pane startup; `service.rs:210-227` sends no pinned skill plan;
  `harness/launch.rs:305-315` reloads the catalog and operator dir in the
  worker. A sync that changes SKILL.md between the ledger write and worker
  start runs unrecorded content. After O1 (`skills/catalog.rs:389-391`
  skips a missing source), a deleted skill silently disappears while the
  ledger says it ran.
- Preferred fix: stage one immutable bundle (copy of each resolved skill,
  digest-checked) before the ledger write, and launch that exact bundle.
  Minimum fix: pass the resolved refs (name, version, digest) through the
  Brief and fail the launch on any mismatch or missing skill.
- The planning code must stay free of I/O (ARC-15): staging happens in the
  service/CLI shell.
- Also fix `teammates/README.md:222-226` (LOW): it still says to add
  `operator_skills` yourself; the roster now sets it. Describe the
  warning-and-skip behaviour and the exact `xcrun agent skills export` command.

## FILES

own: `crates/horch/src/cmd/spawn.rs` (operator part), `execution/service.rs`
(brief/staging), `harness/launch.rs` (the reload), `skills/catalog.rs`,
`skills/materialize.rs`, the coordinator's operator-skill path, tests,
`teammates/README.md` lines 222-226, `ai_docs/reports/finish/operator-digest.md`.

## STEPS

1. Design in the report (which fix, why). 2. Implement with tests: changed
content between spawn and launch fails; deleted skill fails; unchanged
passes. 3. README. 4. Targeted checks; COMMITTED note.
