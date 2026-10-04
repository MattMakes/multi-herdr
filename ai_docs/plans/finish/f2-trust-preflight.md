# F2 trust-preflight: a dataset round never stalls at a trust dialog

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Owner: opus-55 (wrote T5, owns preflight).

## GOAL

1. Preflight refuses a round (exit 4, before any worktree or model call)
   when a candidate harness has not trusted the target repo, and prints the
   exact one-time command to fix it.
2. If a candidate pane still shows a trust dialog, the coordinator detects
   it within one tick and records the candidate as blocked with the reason,
   instead of waiting for the deadline.

## CONTEXT

- Evidence from L1 (opus-61, `ai_docs/reports/finish/acceptance-dataset.md`
  when written; ask opus-61 for details): Claude and Codex key trust on the
  **main repo root** of a worktree, not on a parent folder. `~/.claude.json`
  `projects["<root>"].hasTrustDialogAccepted` (check the exact key in the
  file and Claude Code docs) and `~/.codex/config.toml`
  `[projects."<root>"] trust_level = "trusted"` (check codex docs). Read the
  files; never write them (the operator accepts trust).
- Never print anything else from those files; they can hold tokens.
- Pane detection: read the pane (`herdr pane read`) and match the dialog
  texts ("Is this a project you created or one you trust?", codex's
  "Trusting will apply to the repository root"); keep the strings in one
  place with a test.
- Other harnesses (opencode, pi, prime, agy): find whether they have a trust
  step; report.

## FILES

own: `crates/horch-core/src/competition/preflight.rs`, `crates/horch/src/dataset/preflight.rs`,
the coordinator's candidate-observe path, the dataset design §3 table row
(new PRE id), tests, `ai_docs/reports/finish/trust-preflight.md`.

## STEPS

1. Preflight check + tests (trusted, untrusted, missing files). 2. Pane
detection + tests (FakeWorkspace text). 3. Design text. 4. Targeted checks; COMMITTED.
