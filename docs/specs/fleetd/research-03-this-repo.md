## Summary

Scope: this repository (`/Users/MASCOTT/projects/mh/multi-herdr`), `main` at `cb9655c6`, and the task ledger `/Users/MASCOTT/.horch/ledger.sqlite3` (opened read-only, `mode=ro`). Measured on 2026-11-11.

- Q1 hot files. Since 2026-09-01, `main` has 37 idea-level commits (51 first-parent commits, minus 13 `Release v` commits and `init`). `crates/horch-core/tests/golden_prompts.rs` and `README.md` change in 20 of 37 ideas. `crates/horch/src/main.rs` changes in 17 of 37. Across all 1479 non-merge commits, `skills/README.md` has 70 commits and `crates/horch-core/src/roster/validation.rs` has 62 (F1, F2).
- The hot files are shared registries and indexes: the CLI entry point, roster rules, prompt goldens, skill indexes, the changelog (F2).
- Q2 overlap today. `Board::file_held` (`tasks.rs:806`) holds a queued task when a running task of the same run owns an overlapping path. The board loads only 1 run (`tasks.rs:723`). There is no check across runs, so 2 ideas never wait for each other (F3).
- **Own** paths are not enforced at land. A task with no **Own** rows never waits (F3, F6).
- Landing rebases the task branch on the run branch, runs the check, then fast-forwards under a lock (`land.rs:454`, `land.rs:703`, `land.rs:734`). A conflict aborts the rebase, records `land_red` and gives the conflict back to the same worker (F4).
- Q3 cost of serialization. The ledger does not record the wait reason. I reconstructed it from claim times. In this repo's 5 runs with 3 or more tasks, 0 tasks were held by an **Own** overlap. In the last 10 runs of the whole ledger, 2 tasks were held, 31.0 min in total (F5).
- 3 of the 5 runs of this repo have 0 **Own** rows, so the serialization rule was not active for 33 of 49 tasks (F6).
- Q4 conflicts. In this repo's ledger, 3 of 49 land attempts stopped on a rebase conflict, 3 of 44 landed tasks (6.8%). In the whole ledger, 10 rebase conflicts against 376 landed tasks (2.7%). Each was resolved by the same worker, who rebased by hand and kept both sides (F7, F8).
- In 1 run without **Own** rows, 8 tasks changed `scripts/live/video.sh` and only 1 land conflict on that file is recorded (F8).
- In git, 12 of 238 merge commits since 2026-09-01 hold a conflict resolution. 4 were in skill indexes (`skills/README.md`, `skills/provenance.json`), 8 in Rust code during the big-release run. All were "keep both sides" or "take the newer side" (F9).
- Q5 test tiers. There is no separate unit command. `cargo test --workspace` runs unit and E2E tests together. The E2E crate is `horch-e2e` (`cargo test -p horch-e2e`). On CI run 42948453536 (ubuntu), the test step took 5 min 4 s: compile 1 min 30 s, E2E binaries 139.1 s for 353 tests, all other binaries 72.3 s for 2485 tests (F10).
- The ledger land step for this repo: median 61 s, p90 1178 s, max 8455 s over 44 lands (F11).
- Q6 structure. The 12 hottest Rust files hold 26 to 145 top-level items each. 4 of them hold 1371 to 1794 lines of `#[cfg(test)]` code. Conflicts that occurred were in appended list entries and in 1 shared function body, not in item headers (F12, F9).

Numbers that bear most on the decision:
1. Same-file overlap between ideas is the normal case: the hottest files change in 46% to 54% of all ideas (F1).
2. Textual land conflicts are rare: 2.7% of landed tasks over the whole ledger, 6.8% in this repo (F7).
3. Today's **Own** serialization cost little: 2 held tasks and 31.0 min in the last 10 runs (F5). It also does not apply across runs (F3).

## Method

Commands run from the repository root unless stated. The ledger was opened with `sqlite3 "file:/Users/MASCOTT/.horch/ledger.sqlite3?mode=ro"` and with Python `sqlite3.connect(..., uri=True)` on the same `mode=ro` URI. Nothing was written to the ledger. The analysis scripts are in `/tmp/r3/` and are not part of the repo.

## Q1: hot files

### F1. Ideas per file on `main` (verified)

An idea reaches `main` as 1 squash commit (for example `8e315e04 solo-w2X: fleet run (#347)` has 1 parent). So each first-parent commit is 1 idea.

```
for h in $(git log main --first-parent --since=2026-09-01 --format='%h %s' | grep -v ' Release v' | cut -d' ' -f1); do git diff --name-only $h^1 $h; done | sort | uniq -c | sort -rn | head -12
```

Result: 38 commits, the oldest (`a1e67984 init`) has no parent, so 37 ideas. Median files per idea: 34, max 1608.

| Ideas (of 37) | File |
|---|---|
| 20 | `README.md` |
| 20 | `crates/horch-core/tests/golden_prompts.rs` |
| 17 | `teammates/README.md` |
| 17 | `teammates/_base/fleet-orchestrator.md` |
| 17 | `crates/horch/src/main.rs` |
| 14 | `teammates/_template.md` |
| 14 | `crates/horch-core/src/roster/validation.rs` |
| 14 | `CHANGELOG.md` |
| 13 | `crates/horch/src/cmd/recipes.rs` |
| 12 | `skills/README.md` |
| 12 | `skills/orchestrate/SKILL.md` |
| 12 | `crates/horch-core/src/harness/launch.rs` |

### F2. The 30 files with the most commits (verified)

Command (all non-merge commits reachable from `main`, which includes task commits of fleet branches):

```
git log main --since=2026-09-01 --format= --numstat | awk '$1!="-"{a[$3]+=$1+$2; c[$3]++} END{for(f in c) print c[f], a[f], f}' | sort -rn | head -30
```

Current line count: `git show main:<file> | wc -l`. "absent" means the file is not on `main` now. `git rev-list --count main --since=2026-09-01` gives 1717 commits, of which 1479 are non-merge and 238 are merges.

| # | Commits | Lines added + deleted | Lines now | File |
|---|---|---|---|---|
| 1 | 70 | 904 | 344 | `skills/README.md` |
| 2 | 62 | 4614 | 3362 | `crates/horch-core/src/roster/validation.rs` |
| 3 | 59 | 17434 | absent | `skills/provenance.json` |
| 4 | 55 | 4908 | 3030 | `crates/horch-core/src/harness/launch.rs` |
| 5 | 53 | 2122 | 1580 | `crates/horch-core/tests/skills_catalog.rs` |
| 6 | 43 | 2882 | 1794 | `crates/horch/src/main.rs` |
| 7 | 43 | 1730 | 1318 | `README.md` |
| 8 | 42 | 962 | 694 | `teammates/README.md` |
| 9 | 42 | 4137 | 2449 | `crates/horch/src/cmd/recipes.rs` |
| 10 | 42 | 3605 | 2557 | `crates/horch-core/src/execution/lifecycle.rs` |
| 11 | 42 | 3005 | 1809 | `crates/horch/src/cmd/messaging.rs` |
| 12 | 40 | 6722 | 4002 | `crates/horch/src/cmd/taskledger.rs` |
| 13 | 40 | 4647 | 3411 | `crates/horch-core/src/harness/codex.rs` |
| 14 | 40 | 3164 | 1580 | `crates/horch/src/cmd/spawn.rs` |
| 15 | 40 | 2536 | 2162 | `crates/horch-core/tests/golden_prompts.rs` |
| 16 | 40 | 183 | 85 | `crates/horch-core/src/lib.rs` |
| 17 | 36 | 4394 | 4030 | `crates/horch-e2e/tests/taskledger.rs` |
| 18 | 35 | 625 | 441 | `teammates/_template.md` |
| 19 | 35 | 409 | 375 | `CHANGELOG.md` |
| 20 | 35 | 226 | absent | `ai_docs/plans/arch-refactor-dataset/STATUS.md` |
| 21 | 34 | 2964 | 2008 | `crates/horch/src/cmd/doctor.rs` |
| 22 | 33 | 815 | 735 | `crates/horch-core/tests/baseline_oracles.rs` |
| 23 | 32 | 3064 | 1724 | `crates/horch-core/src/telemetry/collect.rs` |
| 24 | 30 | 4242 | absent | `crates/horch/src/dataset/run.rs` |
| 25 | 30 | 2205 | 1805 | `docs/specs/architecture.md` |
| 26 | 29 | 2677 | 2509 | `Cargo.lock` |
| 27 | 28 | 5240 | absent | `crates/horch-core/src/competition/coordinator.rs` |
| 28 | 28 | 1526 | 512 | `crates/horch-core/src/skills.rs` |
| 29 | 27 | 946 | 578 | `crates/horch-core/src/harness/mod.rs` |
| 30 | 27 | 683 | 423 | `teammates/_base/fleet-orchestrator.md` |

Kinds of hot file (from the names and F9): indexes and registries (`skills/README.md`, `skills/provenance.json`, `teammates/README.md`, `lib.rs` with 26 `mod` lines), the CLI entry point (`main.rs`), roster rules (`validation.rs`), harness prompt builders (`launch.rs`, `codex.rs`) and their goldens (`golden_prompts.rs`, `baseline_oracles.rs`).

## Q2: how the code handles overlap today

### F3. The overlap decision (verified)

- `paths_overlap` decides that 2 paths overlap: equal, or 1 is a directory that holds the other. `crates/horch-core/src/taskledger/util.rs:67`.
- `Board::file_held` holds a queued task when a running or landing work task owns an overlapping path. `crates/horch-core/src/taskledger/tasks.rs:806`. It is called from `Board::wait`, `tasks.rs:769`, after `exclusive` and `shared_checkout`. Review tasks never wait on files.
- Scope is 1 run. `Board::load` reads **Own** rows with `WHERE t.run_id = ?1 AND f.mode = 'own'` (`tasks.rs:723`). Tasks of 2 different runs are never compared. 2 ideas in 2 runs can change the same file at the same time.
- **Own** is used only for scheduling. A search for `mode = 'own'` and `FileMode::Own` in `crates/horch-core/src` finds the scheduler query and the insert (`tasks.rs:567`). No finish step compares the changed files with **Own**.
- The wait is computed live and is not stored: `Wait::FileHeld` is displayed as "`<path>` is held by `<task>`" (`tasks.rs:671`). The `events` table has 0 rows with that text that come from the scheduler (the 2 matches are free-text worker notes).

### F4. Landing on the run branch (verified)

- `land` (`crates/horch-core/src/taskledger/finish/land.rs:454`): rebase on the run branch, run the check, fast-forward the run branch under the land lock, push.
- `rebase` (`land.rs:703`) runs plain `git rebase <run branch>`. It skips the rebase when the run branch is already an ancestor (`land.rs:711`), so a branch with merge commits is not flattened.
- `rebase_and_check` (`land.rs:634`) runs the rebase and the check outside the lock. Under the lock, it lands only if the run branch did not move. Otherwise it tries again, at most `MAX_LAND_TRIES = 5` (`land.rs:27`).
- `fast_forward` (`land.rs:729`) uses `git merge --ff-only` when the run branch is checked out (`land.rs:734`), else `git update-ref` with the old value as a compare-and-swap.
- On a conflict: `git rebase --abort` (`land.rs:718`), the log is written, and the stage is red with "rebase on `<branch>` conflicts; resolve it with git rebase `<branch>` in `<dir>`, commit, then run finish again". `red` (`land.rs:753`) sets the worktree `open` and records a `land_red` event (`land.rs:770`). The same worker resolves it. No other agent is involved.
- A run branch reaches `main` through 1 PR, merged as 1 squash commit (F1).

## Q3: cost of serialization

### F5. Tasks held by an Own overlap (reconstructed, verified from ledger rows)

The ledger does not record why a queued task waited (F3). I could not determine the held time directly. I reconstructed it: for each claimed work task T, the eligible time E is the later of `created_at` and the `finished_at` of each blocker. The held time is the part of [E, first claim of T] during which a work task of the same run with an overlapping **Own** path was between its first claim and its `finished_at`. Other causes of delay in that window (no free worker, an exclusive task) are counted too, so these values are an upper bound. Script: `/tmp/r3/held.py`.

This repo, all runs with 3 or more tasks (5 runs):

| Run | Tasks | Tasks with Own | Held tasks | Held time |
|---|---|---|---|---|
| fleetd-design | 3 | 3 | 0 | 0 min |
| video-producer | 22 | 0 | 0 | 0 min |
| video-pipeline | 13 | 10 | 0 | 0 min |
| blender-teammates-b | 8 | 0 | 0 | 0 min |
| blender-teammates | 3 | 0 | 0 | 0 min |

The ledger holds only these 8 runs for this repo, from 2026-11-02. Earlier fleet work has no ledger rows (not verified why). I could not determine serialization cost before 2026-11-02.

Whole ledger, the last 10 runs with 3 or more tasks (all projects, newest first):

| Run | Project | Tasks | Tasks with Own | Held tasks | Held time |
|---|---|---|---|---|---|
| fleetd-design | multi-herdr | 3 | 3 | 0 | 0 min |
| solo-wFM | v1/essentials-marketplace | 20 | 15 | 2 (T-7, T-18) | 31.0 min |
| ser-model-refresh-5-5-v2 | v5/essentials-marketplace | 44 | 44 | 0 | 0 min |
| ser-model-refresh-5-5 | v5/essentials-marketplace | 86 | 86 | 0 | 0 min |
| deflake | v6/SlipStreamKnowledgeStore | 3 | 3 | 0 | 0 min |
| slippy-dev-factcheck | v5/SlipStreamKnowledgeStore | 7 | 6 | 0 | 0 min |
| solo-wF8 | v6/SlipStreamKnowledgeStore | 32 | 0 | 0 | 0 min |
| solo-wF6 | essentials-marketplace | 20 | 0 | 0 | 0 min |
| solo-wF5 | essentials-marketplace | 4 | 0 | 0 | 0 min |
| solo-wF1 | SlipStreamKnowledgeStore | 4 | 0 | 0 | 0 min |

Total: 2 held tasks, 31.0 min, in 223 tasks. Older runs show larger single holds: `fact-check-slippy` 2 tasks, 1530.7 min; `embed-5-side-by-side` 1 task, 1149.0 min. Those runs hold many review tasks, and the window also counts time with no free worker.

Shared **Own** paths in these runs are mostly index files and directories: `evaluations/README.md` (3 tasks), `plugins/essentials/README.md` (3 tasks), `slipstream-ui` (3 tasks). Query: self-join of `task_files` on equal `path` within 1 run. In each of these pairs, `task_deps` has no edge between the 2 tasks.

### F6. Own rows are often absent (verified)

```
select r.slug, count(distinct t.task_id), count(f.path) from runs r join tasks t on t.run_id=r.run_id left join task_files f on f.task_id=t.task_id and f.mode='own' group by r.run_id
```

In this repo, video-producer (22 tasks), blender-teammates-b (8) and blender-teammates (3) have 0 **Own** rows. 33 of 49 tasks in this repo's runs with 3 or more tasks could not be serialized by file. In the whole ledger, 4 of the last 10 runs with 3 or more tasks have 0 **Own** rows.

## Q4: conflicts that occurred

### F7. Land outcomes in the ledger (verified)

```
select kind, count(*) from events where kind in ('land_started','landed','land_red','unlanded') group by kind
```

| Scope | land_started | landed | land_red | rebase conflict | red check after rebase | run branch moved |
|---|---|---|---|---|---|---|
| this repo | 49 | 44 | 5 | 3 | 2 | 0 |
| whole ledger | 454 | 376 | 15 | 10 | 2 | 3 |

Rebase conflicts per landed task: 3 of 44 (6.8%) in this repo, 10 of 376 (2.7%) in the whole ledger. The `unlanded` events (14) are review or report tasks with nothing to land, and 1 push that failed on a diverged base. None is a file conflict.

### F8. Each land conflict in this repo (verified unless stated)

| Run, task | When | File | How it was resolved |
|---|---|---|---|
| video-producer T-1 (merge fleet/video-pipeline into the run branch) | 2026-11-04 23:02 | The land conflict came from `git rebase` flattening a merge commit, not from 2 tasks on 1 file. Before the land, the task resolved 6 merge conflicts: `crates/horch-core/src/roster/validation.rs`, `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, `crates/horch-core/tests/skills_catalog.rs`, `crates/horch-core/tests/oracles/launch/orchestrator.json`, `docs/skills-and-teams.md`, `skills/README.md` | The worker kept both sides in each file. The operator fast-forwarded the run branch. T-3 then changed `land.rs` to skip the rebase when the run branch is an ancestor (F4). |
| video-producer T-3 | 23:45 and 00:40 | No conflict. `cargo test --workspace` failed after the rebase. The worker reported a check timeout under machine load. | Landed at 01:58 after the timeout was raised. |
| video-producer T-8 | 2026-11-05 01:05 | `scripts/live/video.sh` | The worker rebased by hand and kept both self-test blocks, then landed 7 min later (01:12). |
| video-producer T-12 | 01:25 | I could not determine the file. The event text has no file name. T-12 and T-8 both changed `scripts/live/video.sh` (not verified as the conflict file). | The worker rebased by hand and landed 10 min later (01:35). |

The 3 conflicts in this repo cost 7 to 10 min each from `land_red` to `landed`, for T-8 and T-12.

Files changed by 2 or more tasks of 1 run branch, from the diff between consecutive landed commits (script `/tmp/r3/files.py`):

- `fleet/blender-teammates` (runs blender-teammates, blender-teammates-b, video-producer): 29 lands, 274 files, 66 files changed by 2 or more tasks. `scripts/live/video.sh` was changed by 8 tasks. `skills_catalog.rs` and `docs/skills-and-teams.md` by 6 each. T-1 is a merge of another run branch, so its diff is large.
- `fleet/video-pipeline`: 13 lands, 60 files, 22 files changed by 2 or more tasks, 0 land conflicts.

So 66 multi-task files on 1 branch, with tasks running in parallel and without **Own** rows, gave 3 land conflicts.

### F9. Conflict resolutions in git merge commits (verified)

```
for h in $(git log main --merges --since=2026-09-01 --format=%h); do git show --remerge-diff --format= $h | grep -c '^diff --git'; done
```

12 of 238 merge commits have a non-empty remerge diff. Rebased lands leave no trace in git, so F7 is the only record of those.

| Merge | Date | Conflict files | Resolution (changed lines in remerge diff) |
|---|---|---|---|
| `e4b0262f` Merge D01 skill-taste | 2026-10-03 | `skills/README.md` | both sections kept (3) |
| `854f9751` Merge D04 skill-design-system | 2026-10-03 | `skills/README.md`, `skills/provenance.json` | both kept (300) |
| `c59beff5` Merge S1 swift-core | 2026-10-03 | `skills/README.md`, `skills/provenance.json` | both kept (8) |
| `12f70e40` Merge W1 ue-vendor | 2026-10-03 | `skills/README.md`, `skills/provenance.json` | both kept (71) |
| `8c5cb40c` Merge W2 ue-own | 2026-10-03 | `skills/README.md` | both kept (3) |
| `ae037716` Merge S4 swift-ship | 2026-10-03 | `skills/README.md` | both kept (3) |
| `f4b94594` big-release into pr-267 | 2026-10-30 | `crates/horch-core/src/workspace/herdr.rs` | 6 |
| `e498d6e5` big-release into pr-261 | 2026-10-30 | `crates/horch/src/cmd/taskledger.rs` | 9 |
| `7dd674f5` big-release into pr-265 | 2026-10-30 | `crates/horch/src/cmd/taskledger.rs` | the newer side kept, which holds both changes (14) |
| `33f76a17` big-release into the T-1 branch | 2026-10-30 | `finish/land.rs`, `tests/taskledger_queue.rs`, `horch-e2e/tests/taskledger.rs` | 2714 |
| `25ab1e50` big-release into i308-worktree-triage | 2026-10-30 | `crates/horch/src/cmd/worktree.rs` | 26 |
| `da6ac908` big-release into w5-test-hygiene | 2026-10-30 | `crates/horch-core/src/taskledger/workers.rs` | the refactored SQL kept with the new return value (8) |

Example, `ae037716` (`skills/README.md`): 2 waves each appended a new section at the same place in the file. The resolution removed the markers and kept both sections. Example, `da6ac908`: 1 side changed `conn.execute(` to `let n = conn.execute(`, the other moved the SQL into `format!`. The resolution combined both.

Of the 5 first-parent merges on `main`, 1 (`015f567a`, PR #332) had 2 files changed on both sides, and 0 had a conflict.

## Q5: test tiers

### F10. Commands and CI times (verified)

- Unit and E2E are not separate commands. `cargo test --workspace` runs both (`justfile` recipe `verify`, `.github/workflows/ci.yml` job `test`, gate step 8 in `docs/testing-and-gates.md`).
- E2E tests are the crate `horch-e2e`. `docs/testing-and-gates.md:185`: "Run `cargo build --workspace --bins` before `cargo test -p horch-e2e`."
- A unit-only command would be `cargo test --workspace --exclude horch-e2e`. No file of the repo names it (not verified as passing).
- The full gate is `just gate` (`scripts/phase-gate.sh`, 13 steps). `docs/testing-and-gates.md:13`: "On a full machine it takes about 4 minutes." (not verified).

Newest CI run: 42948453536, `feat/game-design-juicer`, 2026-11-11, success. Command: `gh run view 42948453536 --log`. I split the per-binary `test result: ... finished in <s>` lines by the `horch-e2e` block of the output.

| Job | Step time | Compile | E2E binaries | E2E tests | E2E time | Other binaries | Other tests | Other time |
|---|---|---|---|---|---|---|---|---|
| test (ubuntu-latest) | 5 min 4 s | 1 min 30 s | 41 | 353 | 139.1 s | 86 | 2485 | 72.3 s |
| test (macos-15) | 7 min 17 s | 2 min 9 s | 42 | 355 | 184.1 s | 85 | 2483 | 117.7 s |

Other CI jobs of the same run: `release-lint` (clippy release) 66 s, `docs` 29 s, `build-windows` 1 min 59 s. Earlier CI runs took 6 min 27 s to 6 min 48 s wall time (`gh run list --workflow ci.yml`).

E2E is 12% of the tests and 66% (ubuntu) or 61% (macOS) of the test run time.

### F11. Land check times in the ledger (verified)

```
select s.name, (julianday(s.ended_at)-julianday(s.started_at))*86400 from task_steps s ... where s.status='passed' and s.name in ('check','land')
```

For this repo: `land` step, 44 passes, median 61 s, p90 1178 s, max 8455 s. `check` step, 30 passes, median 207 s, p90 1078 s, max 6878 s. Each land runs the task check again after its rebase (F4). The task checks of this repo range from a 1-file self-test (`python3 skills/video-qa/scripts/qa_video.py --self-test`) to `cargo test --workspace`.

## Q6: structure of the hot files

### F12. Top-level items and test lines (verified)

Command: `git show main:<file>` through an awk script that counts column-0 `fn`, `struct`, `enum`, `impl` and `mod` lines (with `pub` and `pub(...)` prefixes) and the lines of each column-0 `#[cfg(test)] mod x { ... }`. Items made by macros are not counted. 7 files have 42 or more commits. 5 files tie at 40 commits, so the table has 12 files.

| Commits | File | Lines | fn | struct | enum | impl | mod | Top-level items | `#[cfg(test)]` lines |
|---|---|---|---|---|---|---|---|---|---|
| 62 | `crates/horch-core/src/roster/validation.rs` | 3362 | 39 | 0 | 0 | 4 | 2 | 45 | 1471 |
| 55 | `crates/horch-core/src/harness/launch.rs` | 3030 | 42 | 6 | 1 | 2 | 1 | 52 | 1724 |
| 53 | `crates/horch-core/tests/skills_catalog.rs` | 1580 | 50 | 0 | 0 | 0 | 0 | 50 | whole file is a test |
| 43 | `crates/horch/src/main.rs` | 1794 | 13 | 1 | 14 | 8 | 3 | 39 | 455 |
| 42 | `crates/horch/src/cmd/recipes.rs` | 2449 | 46 | 4 | 2 | 6 | 1 | 59 | 995 |
| 42 | `crates/horch-core/src/execution/lifecycle.rs` | 2557 | 36 | 4 | 4 | 3 | 1 | 48 | 1371 |
| 42 | `crates/horch/src/cmd/messaging.rs` | 1809 | 43 | 3 | 0 | 3 | 1 | 50 | 531 |
| 40 | `crates/horch/src/cmd/taskledger.rs` | 4002 | 129 | 7 | 5 | 3 | 1 | 145 | 893 |
| 40 | `crates/horch-core/src/harness/codex.rs` | 3411 | 69 | 4 | 2 | 6 | 1 | 82 | 1794 |
| 40 | `crates/horch/src/cmd/spawn.rs` | 1580 | 34 | 6 | 0 | 2 | 2 | 44 | 455 |
| 40 | `crates/horch-core/tests/golden_prompts.rs` | 2162 | 26 | 0 | 0 | 0 | 0 | 26 | whole file is a test |
| 40 | `crates/horch-core/src/lib.rs` | 85 | 1 | 0 | 0 | 0 | 26 | 27 | 12 |

- Each hot source file has 39 to 145 top-level items, so 2 changes to 1 file can fall in different items.
- In `validation.rs`, `launch.rs`, `lifecycle.rs` and `codex.rs`, 44% to 57% of the lines are in the test module. A new test is usually appended at its end, which is the same place for every task.
- `main.rs` has 14 enums (the CLI argument types). A new subcommand changes the same enum.
- `lib.rs` is a list of 26 `mod` lines. A new module adds 1 line to the same list.
- The conflicts in F8 and F9 were in appended list entries (`skills/README.md`, `skills/provenance.json`, `scripts/live/video.sh` self-test blocks) and in 1 shared function body (`workers.rs` `agent_started`).

## What the facts mean for options A to E

Option A, rebase and re-verify. The land path already does this per task: rebase, run the check again, fast-forward (F4). Across the whole ledger, 2.7% of landed tasks hit a rebase conflict (F7), and every recorded conflict was resolved by keeping both sides or the newer side (F8, F9). The cost of each re-verify depends on the check: the median land step is 61 s and the p90 is 1178 s (F11). The full suite takes 5 to 7 min on CI (F10). The current code gives a conflict back to the same worker, not to an integrator (F4).

Option B, combine at the file. The hottest files change in 46% to 54% of ideas (F1). A per-file combine of every hot file would therefore merge many ideas into 1 task. The hot files are registries and large multi-item files with 39 to 145 top-level items (F2, F12), so most changes to them do not touch the same item. In the 1 run where 8 tasks changed 1 file in parallel, 1 land conflict on that file is recorded (F8).

Option C, integration lane. The run branch of today is a lane of this kind within 1 run: tasks land 1 at a time under a lock, each gated by its own check (F4). The lane retries when the branch moves, at most 5 times (F4). E2E is 61% to 66% of the test run time (F10), which supports moving E2E off each landing. The repo has no unit-only command today (F10).

Option D, structural merge. The conflicts found were appends at the same place in Markdown indexes, JSON registries and a shell script, plus 1 Rust function body (F8, F9). A syntax-aware tool for Rust does not cover the Markdown, JSON and shell cases, which are 3 of 10 distinct conflict files and 7 of 13 conflict events in F8 and F9. Every resolution kept both sides or the newer side (F9).

Option E, a mix. The facts show 3 separate layers. Overlap across ideas is common (F1) and is not checked today (F3). Textual conflicts at land are rare (F7) and most are append-style (F9, F12). The test cost splits into a fast unit part and a slow E2E part (F10). **Own** serialization within 1 run cost 31.0 min in the last 10 runs (F5) and was absent for most tasks (F6).
