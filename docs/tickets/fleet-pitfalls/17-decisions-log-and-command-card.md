# 17: Orchestrator decisions log and a command card on resume (P22, P31)

**Issue:** [#57](https://github.com/MattMakes/multi-herdr/issues/57) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo test -p horch-core --test golden_prompts --test context_roster && cargo test -p horch compact && env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** The context-message keys, the resume line (1 for every session) and the orchestrator's self-handoff and compact instructions are unchanged. None of them names a decisions log or the run.
- **The task ledger now holds much of what the orchestrator used to keep in its head:** tasks, who holds each one, why queued tasks wait, workers and worktrees (`horch ledger status`). It does not hold:
  - the run slug;
  - the operator's vetting decisions;
  - open questions and their answers;
  - the orchestrator's own decisions.

  The decisions log is for these.
- **Where to keep the log:** the run slug matches the run's plan folder, `ai_docs/plans/<run-slug>/` (local scratch, not in git). Keep the log there as `decisions.md`.
- **Text to update:** the self-handoff currently says to keep "every live role from horch sessions, its plan file", and the compact keep-list keeps "the roster" and "the ownership map". In a ledger run, keep the run slug, and rebuild the roster and ownership from `horch ledger status`.

## What to build

An orchestrator that compacts keeps what it decided and how to drive horch.

- **Decisions log.**
  - The orchestrator keeps `decisions.md` in the run's plan folder: the decision, the reason, and the options the operator rejected.
  - The orchestrator briefing says to add to it whenever the operator decides, and to read it before it proposes a design.
  - The self-handoff and the compact instructions both name it, and the run slug.
  - In a ledger run, they say to rebuild the roster and ownership from `horch ledger status`, instead of keeping them.
- **Command card.**
  - A new orchestrator-only resume message carries a 1-line card of the horch command forms: `horch ledger status`, `horch ledger plan-workers`, `horch spawn`, `horch tell`, `horch inbox`, `horch ledger events`, `horch context --over`, `horch compact`, `horch note --fact`.
  - Workers keep the current resume line.
  - Context messages stay 1 line each.

## Acceptance criteria

- [ ] Compacting the orchestrator types the orchestrator resume message, with the command card; compacting a worker types the worker one.
- [ ] `teammates --check` requires the new message key, and fails on an overlay that lacks it.
- [ ] The orchestrator golden, the pinned context-watch text and the keep-list assertion are updated in named sanctioned blocks. The Claude and Codex orchestrators still differ only in their tier block.

## Blocked by

None (can start immediately)
