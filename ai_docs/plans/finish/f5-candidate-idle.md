# F5 candidate-idle: Codex candidates in worktrees, and idle candidates

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Owner: opus-61 (found it in LA runs).

## GOAL

1. A Codex candidate in a dataset round finishes its task without needing to
   write `.git` (the Codex sandbox blocks `.git/worktrees/<name>`, a known
   limit: memory "Codex workers cannot write .git"). The coordinator already
   freezes (commits) each candidate's work: make the candidate brief say
   "do not commit; the coordinator commits your work", check the freeze
   path handles uncommitted and untracked files, and test it with a fake
   codex that tries `git commit` and fails.
2. A candidate that is idle (herdr `agent_status` idle; opus-62 added the
   field to `Pane`) without `horch done` for a set time gets one nudge (a
   prompt: "If you are finished, run horch done ...") and, if still idle
   after a second period, ends as `idle_without_done` instead of waiting for
   the deadline. Choose the periods with a reason (default deadline 900 s);
   make them config keys in `.multi-herdr/dataset.yaml`; test with
   FakeWorkspace.
3. Decide whether a fleet Codex worker should get the same nudge (horch
   worker), and either do it or record why not.

## CONTEXT

- Evidence: `ai_docs/reports/finish/acceptance-dataset.md` (codex-terra
  blocked on `.git/worktrees/B`, idled 900 s).
- Never widen the Codex sandbox to the repository's common `.git`: a
  candidate could then rewrite other branches.

## FILES

own: the candidate brief template (find it: `teammates/` or the coordinator),
`competition/coordinator.rs` (observe + idle), `competition/config.rs`,
the dataset design section for candidate lifecycle, tests,
`ai_docs/reports/finish/candidate-idle.md`.

## STEPS

1. Brief + freeze test. 2. Idle nudge/end + config + tests. 3. Design text.
4. Targeted checks; COMMITTED.

## Added items (from F2, opus-55's report `ai_docs/reports/finish/trust-preflight.md`)

4. PRE-14 reads `~/.claude.json` only; honour `CLAUDE_CONFIG_DIR` (Claude
   keeps its config there when set; check the docs for the exact file) via
   RuntimeContext. Test both. You also own `competition/preflight.rs` and
   `crates/horch/src/dataset/preflight.rs` for this.
5. `crates/horch/src/dataset/run.rs` exits 4 instead of 3 for a plan with 0
   candidates: use the documented exit code (dataset design, exit table) and
   test it.
