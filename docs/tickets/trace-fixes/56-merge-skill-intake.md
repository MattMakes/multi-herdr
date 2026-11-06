# 56: Merge PR #338 (skill-intake) into the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** no
**Kind:** quality
**Review:** none
**Check:** none
**Own:** `.worktrees/_scratch/merge-338`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator: the operator asked that PR #338 (`fleet/skill-intake`, https://github.com/MattMakes/multi-herdr/pull/338) land together with this run in PR #339.
Waits for: 33.

## Goal

1 real merge commit on top of `fleet/trace-fixes` (`8fb3d713` or newer) whose 2nd parent is `origin/fleet/skill-intake` (`0ea4a7bd`). Keep it a merge: do not rebase, squash or cherry-pick, so GitHub marks PR #338 merged when #339 lands. Do not land the merge through `horch ledger finish`, because land rebases; commit nothing in your task worktree. The orchestrator moves the run branch to your merge commit.

## Steps

1. Work only in the scratch worktree `/Users/mascott/projects/multi-herdr/.worktrees/_scratch/merge-338` (detached at the run-branch tip). Run `git fetch origin fleet/skill-intake`, then `git merge --no-ff origin/fleet/skill-intake`. A trial merge gave 15 conflicted files: messaging/mail.rs, roster/validation.rs, cmd/mail.rs, cmd/messaging.rs, cmd/spawn.rs, cmd/taskledger_spawn.rs, tests baseline_oracles.rs, golden_prompts.rs, skills_catalog.rs, e2e fleet18_brief_worktree.rs, e2e mail.rs, docs/skills-and-teams.md, skills/README.md, teammates/README.md, and more.
2. Resolve each conflict so both sides' behaviour holds. This run's rules win where they clash: pull-only dispatch (no task text on spawn, TRF-43), mail ids and keyed outbox (TRF-21, TRF-51), orchestrator lock on the route (TRF-39), session records in the ledger (TRF-45). PR #338's features stay: skill-intake skill, skill-importer and `intake-*` teammates, report-mode sockets, `spawn_only`, sandbox overlay, read-only bundles. If #338 code spawns with task text or writes the JSON session ledger, port it to this run's model. If a conflict needs a product decision, send the orchestrator 1 QUESTION and keep resolving the rest.
3. Merge both CHANGELOG `## Unreleased` entries (keep both bullets).
4. Commit the merge (`git commit --no-edit` is fine; name both PRs in the message). Check `git rev-list --parents -1 HEAD` shows 2 parents and the 2nd is `0ea4a7bd`.
5. Run in the scratch worktree: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --bins`, `cargo test --workspace --no-fail-fast`, `./scripts/check-req-coverage.sh`, `./scripts/check-deps.sh`. Fix failures in follow-up commits on top of the merge (normal commits). The orchestrator runs the full gate afterwards as a separate task.
6. Never run a branch-built horch against the shared ledger: set `HORCH_LEDGER_DB=$(mktemp -d)/ledger.sqlite3` for any manual run.
7. Send `horch tell orchestrator` with the final commit sha and the test result, then write the report.

## Done when

The merge commit and its fix commits pass step 5 in the scratch worktree, and the orchestrator has the sha.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/56.md` (spec section 8): every conflicted file and how you resolved it, and every #338 behaviour ported to this run's model. Then run `horch ledger finish` from your task worktree, which has no change, so land has nothing to rebase (if a step fails for that reason, tell the orchestrator), and when every step passed, run `horch ledger done "<summary>"`.
