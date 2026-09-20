# Brief: make the fleet workspace label work for relative paths

Repo: /Users/mascott/projects/multi-herdr, branch `fleet-efficiency-plan` (confirm with `git branch --show-current`). Prior commit 3debb5b added `fn workspace_label` in `crates/horch/src/cmd/recipes.rs`. It takes the final path component. A relative path such as `.` or `..` has no final component, so `horch fleet --cwd .` falls back to "Herdr Fleet". Fix that.

## Steps
1. In `workspace_label`, resolve the path before reading its name: try `std::fs::canonicalize(path)`; if that fails (path does not exist in a unit test), fall back to `std::env::current_dir().map(|d| d.join(path))` and normalise `.` and `..` components manually (iterate `components()`, skipping `CurDir`, popping on `ParentDir`). Then take `file_name()`. Keep the "Herdr Fleet" fallback for a true root or empty result.
2. Add tests: `.` resolves to the current directory's name; `..` resolves to the parent's name; `some/dir/.` resolves to `dir`; absolute path unchanged; root still falls back. For tests that need a real directory, use `std::env::temp_dir()` and create it with `tempfile` only if the crate already depends on it (check `Cargo.toml`); otherwise use a path built from `current_dir()`.
3. Do not run `cargo fmt` on the crate (5 files have pre-existing diffs). Check only the touched file: `rustfmt --edition 2021 --check crates/horch/src/cmd/recipes.rs`.
4. `cargo build -p horch` (0 warnings), `cargo test -p horch`.
5. Commit on the current branch, including the two brief files `ai_docs/plans/fleet-workspace-label.md` and `ai_docs/plans/fleet-workspace-label-2.md`. Message:
   ```
   Resolve relative paths before naming the fleet workspace

   `horch fleet --cwd .` fell back to the old label because a relative
   path has no final component. The label helper now canonicalises the
   path first and normalises `.` and `..` when the path does not exist.
   ```
6. `git push`. Report with `horch done` in Simplified Technical English: test count, commit hash.

## Out of scope
Nothing else in recipes.rs. Do not touch `.herdr-orchestrator/`.
