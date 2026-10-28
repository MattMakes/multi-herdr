# 21: Worktrees from a PR ref, scratch worktrees, and dropping a worktree (P02)

**Issue:** [#69](https://github.com/MattMakes/multi-herdr/issues/69) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test taskledger --test worktree && cargo test -p horch-core --test golden_prompts --test skills_catalog`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## Background

This was deferred to the task-ledger spec (#28), which closed without it. v0.3.0 did not fix it.

- `horch ledger worktree new <name>` always branches from the run branch, into a `.worktrees/T<seq>-<name>` folder.
- The old `horch worktree` still has only `new`, `land` and `list`.
- There is no way to check out a PR branch or another ref, make a throwaway scratch worktree, or drop a worktree that will not land. So plans told workers to run `git worktree add /tmp/...`, which breaks the briefing's rules and leaves untracked worktrees.
- Today, only the `cleanup` finish step removes a worktree, and only after it lands.
- The ledger's worktrees table already has an `abandoned` status.

## What to build

Workers never need raw `git worktree add`.

- `horch ledger worktree new <name> --from <ref>` checks out a worktree from any ref, such as a PR branch fetched by a Claude worker. It records the ref as its base.
- `horch ledger worktree scratch <name>` makes a throwaway worktree under `.worktrees/_scratch/` that is recorded but never needs to land. It is dropped without finish steps.
- `horch ledger worktree drop <name> --reason "<why>"` removes a worktree that will not land:
  - It refuses when the worktree has commits that exist nowhere else, unless `--keep-branch` is given. In that case it removes the folder and keeps the branch.
  - It never forces.
  - It records the worktree as `abandoned` with the reason.
- `horch done` no longer refuses over a dropped or scratch worktree.
- The worker briefing and the orchestrate skill say: never put `git worktree add` in a plan; use these commands.

## Acceptance criteria

- [ ] `new --from <branch>` makes a worktree on that branch's commit. The ledger shows the ref as its base.
- [ ] `scratch` worktrees never block `done` or `finish`.
- [ ] `drop` refuses over unpushed commits, and leaves everything untouched. `drop --keep-branch` removes only the folder.
- [ ] A dropped worktree shows as abandoned with its reason in `horch ledger status`.
- [ ] Briefing golden changes are in a named sanctioned block, and the skill stays under its size budget.

## Blocked by

None (can start immediately)
