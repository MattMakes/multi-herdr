# F1 operator-digest: a worker runs exactly the skill bytes the ledger recorded

Plan: `ai_docs/plans/finish/f1-operator-digest.md`. Single-branch work on
`design-skills`.

Commits:
- `c5e9944` (sonnet-13, "Spec history: ...") carries by mistake the first
  version of my `install_skills` and `skill_drift` code in
  `crates/horch-core/src/harness/launch.rs`. The orchestrator reported it.
- `ab40a58` "Harness: Fail a worker launch whose skills differ from the
  ledger record": the clippy fixes, the unit test and 2 e2e tests.
- The next commit, "Teammates: Describe the operator-skill warning, skip
  and launch check": `teammates/README.md` and this report.

## The gap (codex-reviewer-2, MEDIUM)

`horch spawn` reads the catalog and the operator directory, plans, and
writes the resolved skill refs (id, version, digest) into the ledger record
(`ExecutionService::spawn` -> `store.set_skills`). The worker in the new
pane then reads the catalog and the operator directory again
(`harness/launch.rs:install_skills`) and copies what it finds now. Its copy
is digest-checked against its own plan, not against the ledger. So:
- a SKILL.md edited between the spawn and the worker's start ran with
  content that the ledger did not record;
- after O1, a skill deleted in that window was silently skipped, while the
  ledger still listed it.

## Design: which fix, and why

I chose the plan's minimum fix, with the ledger record itself as the pin
(not a new Brief field):

- In `install_skills`, after `Bundle::install_from` plans and copies, a
  launch that has a ledger record (`req.record`, every `horch worker`
  launch) reads that record with `ExecutionStore::open_in(ctx).get(record_id)`
  and compares its `skills` with the bundle plan's `activated`.
- `skill_drift(recorded, launched)` compares by id, version and digest. The
  policy and the source label do not count; the bytes do. It names each
  difference: "'<id>' (<version>) is missing now", "'<id>' was <v1>
  (digest <12>) and is <v2> (digest <12>) now", "'<id>' (<version>) was not
  recorded".
- Any difference fails the launch before the agent starts:
  "the skills changed since <record> was spawned: <differences>. Spawn the
  worker again to record the current skills".
- Together with the existing copy check (`materialize.rs`: the copy must
  match the plan's digest, else "the operator directory changed during the
  launch"), the bytes in the bundle equal the digests in the ledger, or the
  launch fails.

Why not the preferred fix (stage one immutable bundle before the ledger
write and launch that bundle):
- The ledger is already the record of truth (SKL-04). Comparing against it
  needs no new state, no new Brief schema field, and no bundle handover
  between 2 processes (the spawner and the pane), and it covers older
  briefs too.
- Staging at spawn would move the bundle directory, its cleanup and the
  briefing render (which needs the catalog) from the worker to the spawner,
  and touch `cmd/spawn.rs`, `service.rs`, `brief.rs` and `lifecycle.rs`.
  The check gives the same guarantee with 1 file.
- Cost: the worker re-reads and re-copies, as before. A failed check leaves
  the record `LaunchFailed` through the existing worker error path.

Scope of the mechanism:
- Operator skills: covered (the 2 e2e tests).
- Marketplace skills: the same gap existed (a reinstall or `marketplace.lock`
  change between spawn and start) and the same check covers it, because it
  compares every activated ref.
- Bundled skills: a `horch` binary rebuilt between spawn and start now
  fails the launch when a bundled skill changed. That is correct: the
  worker would run other bytes than recorded.
- The competition coordinator (`competition/coordinator.rs`) starts
  candidates through `ExecutionService::spawn` and `horch worker`, so it
  gets the check with no change.
- A launch without a record (`horch pane-launch`, recipes) has nothing to
  compare and is unchanged.
- The planning code stays free of I/O (ARC-15): the read is in the harness
  launch shell.

`cmd/spawn.rs`, `execution/service.rs`, `skills/catalog.rs` and
`skills/materialize.rs` did not change.

## Tests

- `harness::launch::tests::skill_drift_names_missing_changed_and_unrecorded_skills`
  (unit): equal sets in any order pass; policy and source do not count;
  missing, changed (also same version label with other bytes) and
  unrecorded skills give the exact messages.
- `crates/horch-e2e/tests/skills_exposure.rs`:
  - `operator_skills_e2e_changed_since_spawn_fails_the_launch`: SKILL.md
    edited after `horch spawn`; `horch worker` fails with "the skills
    changed since" and "'test-modernizer' was operator+"; the fake claude
    never ran.
  - `operator_skills_e2e_deleted_since_spawn_fails_the_launch`: the skill
    directory deleted after the spawn; the worker fails with "is missing
    now"; claude never ran.
  - Unchanged passes: `operator_skills_e2e_exposure_claude`,
    `operator_skills_e2e_exposure_codex`,
    `operator_skills_e2e_ledger_record_lists_them` and the 5 SKL-06 exposure
    tests still pass.
- Also run: `horch-e2e` `lifecycle` (11 pass) and `brief` (1 pass), which
  run `horch worker` with records.
- `cargo clippy -p horch-core -p horch-e2e --all-targets -- -D warnings`:
  clean. `rustfmt --check` on my files: clean.
- On the tip at the time, 2 tests in `harness::launch::tests`
  (`unset_isolation_fields_add_no_flags`,
  `opting_in_to_claudeai_skills_leaves_the_switch_out`) failed because of
  another worker's uncommitted `harness/claude.rs` edit. opus-63 (F3) owns
  those 2 tests. I did not change them.

## README

`teammates/README.md` (the Swift section, old lines 236-240): it said no
Swift teammate sets `operator_skills`. Now it gives the field the 2
teammates set, the exact command
`xcrun agent skills export --output-dir ~/.agents/skills`, the warning and
skip on a host without the export, and the launch check.

## Gotchas

- The e2e tests run the built `horch` binary. Run
  `cargo build --workspace --bins` before `cargo test -p horch-e2e`, or a
  stale binary fails them with "unknown field `operator_skills`".
- `cargo fmt -p horch-core` formats every file in the crate, including
  other workers' uncommitted edits. Run `rustfmt` on your own files only.

## Follow-ups (not done, outside F1's files)

- `README.md` ("Apple's Xcode skills") and `docs/recipes/add-teammate.md`
  do not yet say that a skill changed between spawn and start fails the
  launch.
