# 18: Teammates state their sandbox limits and pre-allow read-only commands (P04, P07)

**Issue:** [#58](https://github.com/MattMakes/multi-herdr/issues/58) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Check:** `cargo test -p horch-core --test baseline_oracles --test golden_prompts --test skills_catalog && env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed.** No Codex teammate's description mentions `.git` or network. Only YAML comments in some teammates do. No Claude teammate has an `allowed_tools` list.
- **New consequence in the task ledger.**
  - The Codex exec policy now allows `horch ledger` outside the sandbox, so `finish` can land and update the PR.
  - But `finish` refuses while the worktree has uncommitted changes, and a Codex worker cannot run `git commit`.
  - So a Codex persona as `--primary` on a ticket that commits code can never pass `finish`.
  - Only code-sweep's reviewer choice takes Codex into account today.
- The place this matters now is the persona choice in `horch ledger add --primary/--fallback` and the persona columns of a ticket map.

## What to build

The orchestrator never gives a worker a step its sandbox or permission mode will refuse.

- **Codex teammates.**
  - Every Codex teammate's description says it cannot write `.git`, so it is not a primary for tickets that commit, and suits review and read-only work.
  - Every one except `codex-network` says it has no network.
  - `codex-network` says it has network but also cannot write `.git`. So fetches, commits and gates go to Claude workers, not to `codex-network`.
  - The orchestrate skill's persona guidance for `ledger add` and ticket maps says the same, in 1 line.
- **Claude teammates.**
  - Claude worker teammates get an `allowed_tools` list of read-only command patterns, for example status, log, diff, list and read commands for git, `horch ledger status/show/steps/events`, and the cloud CLIs the fleet uses.
  - First, a live check on the operator's machine must confirm that Claude's auto mode honors the list. Record the result in the live-check results.
  - If auto mode ignores the list, ship only the descriptions and note why.
- **Optional guard:** `horch ledger add` could warn when the primary is a Codex persona and the ticket has a check command (which means it commits code).

## Acceptance criteria

- [ ] Each Codex teammate's description states its limits, and `teammates --check` passes.
- [ ] The live check's result is recorded.
- [ ] If the list is honored, the launch oracles are blessed for exactly the teammates that changed, and the diff summary shows only `--allowedTools`.
- [ ] No write, push or install command appears in any allow list.
- [ ] Any change to a teammate that the goldens render is in a named sanctioned block. The skill stays under its size budget.

## Blocked by

None (can start immediately; the allow-list half needs the live check)
