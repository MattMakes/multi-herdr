# Brief: name the fleet workspace after the project folder

Repo: /Users/mascott/projects/multi-herdr. Work on branch `fleet-efficiency-plan` (already checked out; confirm with `git branch --show-current`).

## Goal
`horch fleet` creates its herdr workspace with the fixed label `"Herdr Fleet"`
(`crates/horch/src/cmd/recipes.rs:177`). The operator wants the label to be the
name of the project folder the fleet runs in, e.g. `multi-herdr`.

## Steps
1. In `crates/horch/src/cmd/recipes.rs`, the fleet recipe around line 159-182 already has `cwd` (a path). Derive the label from it: the final path component as a string. Write a small helper `fn workspace_label(cwd: &Path) -> String` next to the recipe that returns `cwd.file_name()` lossily converted, and falls back to `"Herdr Fleet"` when there is no file name (e.g. cwd is `/`) or the name is empty. Use it at line 177: `herdr.workspace_create(&workspace_label(&cwd), Some(&cwd), true)?`.
2. Leave line 224 (`"Herdr Orchestration"`, the fixed 5-pane recipe) unchanged. Only the fleet recipe changes.
3. Add a unit test in the same file's `mod tests` (create one if absent) covering: a normal path returns the folder name; a root path returns the fallback; a path with a trailing slash still returns the folder name.
4. Check whether any doc mentions the old label: `grep -rn "Herdr Fleet" README.md herdr-docs/ teammates/ ai_docs/ crates/` and update README.md or teammates text if they say the workspace is called "Herdr Fleet" (do not edit `herdr-docs/`, it is a mirror, and do not edit files under `ai_docs/reports/`).
5. `cargo fmt -p horch` on the touched file only (do not reformat other files; `crates/horch-core/src/agent.rs` has known pre-existing rustfmt diffs, leave it), `cargo build -p horch`, `cargo test -p horch`.
6. Commit on the current branch with message:
   ```
   Name the fleet workspace after the project folder

   horch fleet labelled every workspace "Herdr Fleet". It now uses the
   final component of the working directory, falling back to the old
   label when the path has none.
   ```
7. `git push`. Report with `horch done` in Simplified Technical English: files changed, test count, commit hash.

## Out of scope
No other recipe, no label changes for tabs or panes, no changes to horch-core.
