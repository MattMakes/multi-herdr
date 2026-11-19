# Bring the final DDD model of the design run into the build branch

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `git merge-base --is-ancestor 9a7b28a7813538556e7efe4f52a22fd7902238a5 HEAD`
**Own:** `ddd/`, `docs/specs/fleetd/`

## Goal

The branch of your task contains commit `9a7b28a7` of `fleet/fleetd-design` (the DDD model, round 3,
after its gate), so that each later planner reads the current `ddd/08-code/implementation-plan.md`.

## Steps

1. Run `git fetch origin fleet/fleetd-design`.
2. Run `git merge --no-ff --no-edit 9a7b28a7813538556e7efe4f52a22fd7902238a5`. The commits change
   only `ddd/` and `docs/`. Expect no conflict. If a conflict occurs, do not resolve it: run
   `git merge --abort` and send BLOCKED with the list of files.
3. Run the Check.

## Constraints

- Before the merge, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Change no file by hand.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes.
