# D19 dataset-tidy: the follow-ups D18 reported

Unit slug: `dataset-tidy`. Branch: `ds/dataset-tidy`.

## GOAL

Every follow-up in `ai_docs/reports/design-skills/flake-and-durable.md`
("Follow-ups") is fixed or settled with a recorded reason:
1. The dataset design's layout (line ~88 of
   `ai_docs/designs/2026-10-02-dataset-competition-design.md`) names a round
   projection file that no code writes. Either write it (if a reader needs
   it: check `status`, `rebuild`, `export`) or delete it from the design.
2. A candidate's `horch done` runs `horch tile` / `horch balance` in the
   dataset workspace. Decide if that is right (the dataset workspace has its
   own layout and a `watch` root pane). If it is wrong, make `done` skip the
   re-tile for candidate and judge executions, with a test.
3. fake-herdr breaks its state lock after 10 s of age or 30 s of waiting.
   Check this against D10's fix (each call in its own process group): is the
   break still needed, and are the numbers right under load? Measure.
4. A `tel_session_recorded_when_agent_ends_fast` run leaked a `horch worker`
   and a fake codex process. Find why the test teardown misses them, fix it,
   and add a teardown check that fails a test that leaks a child process.
   (Note: opus-46, unit V3, is root-causing a flake in the same test; read
   `ai_docs/plans/domain-skills/v3-loose-ends.md` and tell the orchestrator
   before you change the same code.)
5. `cargo clippy -p horch-core --all-targets` warnings listed in the D18
   report: fix them all, then add `cargo clippy --workspace --all-targets --
   -D warnings` to `scripts/phase-gate.sh` if the workspace is clean.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md` and the D18
  report. Worktree under `/Users/mascott/projects/multi-herdr/.worktrees/`.

## FILES

own: the design doc, `crates/horch/src/cmd/` (`done` only), the e2e
harness and fakes for items 3–4 (coordinate with V3), clippy fix sites,
`scripts/phase-gate.sh`, `ai_docs/reports/design-skills/dataset-tidy.md`.

do not touch: skills, teammates, roster.

## STEPS

0. Create the worktree. 1–5, a commit each, gate before each commit.
   Report with before/after evidence. Follow the merge protocol.
