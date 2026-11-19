# Worktree cleanup audit (T-2)

Date: 2026-11-19. Base: `origin/main` after `git fetch origin`.
`.worktrees/` size before: **158 GB** (`du -sh .worktrees`). Size after: **168 KB** (only the plain folder `_scratch/` remains). Freed about 158 GB.

Status: done on 2026-11-19. The orchestrator ruled option 1 (operator authorization in the ticket). All 19 worktrees are removed. No branch is deleted.

## Table

"Not on main" = `git rev-list --count origin/main..HEAD`. "Live" = a process has its cwd under the path (`lsof -a -d cwd`). All 19 rows: live = no.

| Worktree | Branch | Size | Dirty | Not on main | Pushed | Live | Class |
|---|---|---|---|---|---|---|---|
| _gate-solo-w2X | detached (tip afadd9da) | 5.0G | 0 | 4 | yes: tip is an ancestor of `origin/fleet/solo-w2X` | no | saved on origin |
| _gate-video-formats | detached (tip cdd7ff71) | 4.3G | 0 | 13 | yes: tip equals `origin/fleet/video-formats` | no | saved on origin |
| T1-task-ledger-runs-and-task-lifecycle-bugs | fleet/big-release-T1-… | 8.8G | 0 | 0 | yes | no | obvious delete |
| T13-refactor-the-competition-module | fleet/big-release-T13-… | 9.1G | 0 | 0 | yes | no | obvious delete |
| T14-refactor-the-evaluation-and-dataset-core | fleet/big-release-T14-… | 8.1G | 0 | 0 | yes | no | obvious delete |
| T15-refactor-the-dataset-cli | fleet/big-release-T15-… | 6.8G | 0 | 0 | yes | no | obvious delete |
| T16-refactor-the-marketplace-crate | fleet/big-release-T16-… | 6.7G | 0 | 0 | yes | no | obvious delete |
| T17-refactor-herdr-install-and-herdr-docs-sy | fleet/big-release-T17-… | 6.2G | 0 | 0 | yes | no | obvious delete |
| T2-finish-pipeline-bugs-land-pr-sweeps-time | fleet/big-release-T2-… | 8.8G | 0 | 0 | yes | no | obvious delete |
| T3-watchdog-liveness-and-process-identity-b | fleet/big-release-T3-… | 9.6G | 0 | 0 | yes | no | obvious delete |
| T33-tests-pass-under-machine-load | fleet/big-release-T33-… | 6.1G | 0 | 0 | yes | no | obvious delete |
| T4-mail-and-message-delivery-races-durabili | fleet/big-release-T4-… | 10G | 0 | 0 | yes | no | obvious delete |
| T5-compaction-requests-that-never-finish-au | fleet/big-release-T5-… | 8.5G | 0 | 0 | yes | no | obvious delete |
| T6-fleet-start-spawn-guards-ledger-isolatio | fleet/big-release-T6-… | 11G | 0 | 0 | yes | no | obvious delete |
| T7-codex-harness-exec-policy-session-discov | fleet/big-release-T7-… | 10G | 0 | 0 | yes | no | obvious delete |
| T9-sessions-and-context-views-note-fact-dec | fleet/big-release-T9-… | 5.9G | 0 | 0 | yes | no | obvious delete |
| T10-codex-workers-can-commit-and-state-their | fleet/fleet-reliability-T10-… | 22G | **11** (10 modified, 1 untracked `crates/horch-core/src/runtime/job.rs`) | 77 | **no** | no | save: commit, push, remove |
| T14-waiting-state-and-open-question-record | fleet/fleet-reliability-T14-… | 8.3G | 0 | 57 | **no** | no | save: push, remove |
| T17-live-checks-standalone-multi-herdr-abort | fleet/skill-intake-T17-… | 2.5G | 0 | 77 | **no** | no | save: push, remove |

Notes:
- `_scratch/` (168 KB) is a plain folder, not a git worktree. It is not in `git worktree list`. No action.
- The 14 "obvious delete" rows have every commit on `origin/main` by ancestry (`git rev-list --count origin/main..HEAD` = 0).
- The 3 "save" rows are not squash-merged: 37 of 39, 64 of 66 and 44 of 139 changed files still differ from `origin/main`. They hold unlanded work.
- `horch worktree list` has 6 rows (rel-u1..u8), all `landed`, none under `.worktrees/`. No row points at a removed path.

## Planned action per row

| Row | Action |
|---|---|
| 14 obvious-delete rows | `git worktree remove` each, keep the branch. Frees about 115 GB. |
| _gate-solo-w2X, _gate-video-formats | Remove the worktree. The tip is on origin already (confirmed with `git ls-remote origin`). Keep the branch. Frees about 9 GB. |
| T10 | Commit the 11 dirty files to its branch, push the branch, confirm with `git ls-remote origin <branch>`, remove the worktree, keep the branch. Frees about 22 GB. |
| T14-waiting, T17-live-checks | Push the branch, confirm with `git ls-remote origin <branch>`, remove the worktree, keep the branch. Frees about 11 GB. |
| End | `git worktree prune`, then `horch worktree list`. |

## Pushed branches

Each remote tip was confirmed with `git ls-remote origin refs/heads/<branch>` before its worktree was removed.

| Branch | Tip on origin |
|---|---|
| `fleet/fleet-reliability-T10-codex-workers-can-commit-and-state-their` | ee444aba (new commit of the 11 dirty files, including untracked `crates/horch-core/src/runtime/job.rs`) |
| `fleet/fleet-reliability-T14-waiting-state-and-open-question-record` | 3247640b |
| `fleet/skill-intake-T17-live-checks-standalone-multi-herdr-abort` | 7c0a88d6 |

The 2 gate tips were on origin already: `fleet/solo-w2X` (tip afadd9da is an ancestor of the remote tip c34c86d5) and `fleet/video-formats` (cdd7ff71).

## Result

- 14 obvious-delete worktrees: removed with `git worktree remove` (no force). Branches kept.
- `_gate-solo-w2X`, `_gate-video-formats`: removed with `git worktree remove` (no force). Branches kept. `horch worktree triage apply` with ARCHIVE failed for both (see bug below), and nothing was deleted by it.
- T10, T14-waiting, T17-live-checks: saved and pushed as above, then removed. Branches kept.
- `git worktree prune`: run. `git worktree list` shows the main checkout only.
- `horch worktree list`: 6 rows (rel-u1..u8), all `landed`, none under `.worktrees/`. Nothing to settle.
- `horch worktree triage`: "No parked worktrees."
- `.worktrees/` after: 168 KB (`_scratch/`, not a worktree, left alone).
- Not removed from `origin`: the 9 `fleet/fleetd-build-T*` branches and `fleet/fleetd-build`.

## Bug found (not fixed, out of scope)

`horch worktree triage apply` with ARCHIVE or DELETE on a detached-HEAD worktree fails: `git rev-list refs/horch/archive/<name>..(detached)` is run with the literal text `(detached)` as the branch. Source: `crates/horch-core/src/taskledger/triage/apply.rs` (`Freeing::prepare` / `archive`). The failure is safe: nothing was deleted.
