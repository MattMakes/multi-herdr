# Single-branch work (from 2026-10-04)

The operator asked for one branch, committed to directly, inside the trusted
project folder. This replaces the per-unit `ds/<unit>` branches and the
merge machinery for all new units.

## Where

- Branch: `design-skills` (PR #18 to `main`).
- Checkout: `/Users/mascott/projects/multi-herdr` itself. No new worktrees.
- Scratch repos for experiments: `/Users/mascott/projects/multi-herdr/.worktrees/_scratch/`
  (git-ignored, inside the trusted folder, so no CLI trust prompt). Never `/tmp`.

## How to commit

- Edit only the files your plan's FILES section gives you.
- Keep the tree compiling at every commit: other workers build the same tree.
  Work in small steps; never leave a half-edited `.rs` file while you wait.
- Commit only your own paths: `git commit -m "<msg>" -- <path> <path> ...`
  (a pathspec commit never picks up another worker's staged or unstaged
  changes). Never `git add -A`, `git add .`, `git commit -a`, `git stash`,
  `git checkout -- <path>` on a file you do not own, `git reset`, or `git rebase`.
- Before you report COMMITTED, run `git show --stat HEAD` and check that
  every file in it is yours. (On 2026-10-04 a commit swept another worker's
  unfinished `launch.rs` edit into an unrelated commit.)
- If git reports `index.lock` exists, wait 2 s and retry (another worker is
  committing). Never delete the lock.
- Do not push. The orchestrator pushes after the full gate passes on the tip.

## Checks

- While working: the tests you touch (`cargo test -p <crate> --test <file>`),
  `cargo clippy -p <crate> --all-targets -- -D warnings`, `rustfmt --check`
  on your files. One shared `target/`, so cargo runs one build at a time.
- Before DONE: the same checks on the current tip, then send
  `[<role>] NOTE: COMMITTED <short-sha>[..<short-sha>]. Targeted checks green. See <report>.`
- The orchestrator runs the full gate on the tip. A red gate goes to the
  worker whose commit broke it, who fixes forward at once (no revert of other
  workers' commits).
- Never run tests or the gate under `git rebase -x` or a git hook.
