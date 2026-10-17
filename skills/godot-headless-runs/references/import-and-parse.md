# Import and parse hygiene

Read this after a new worktree, a merge, a branch switch, a new `class_name` or new art, and when your own files cannot load in the real project.

## Import and parse steps

The `.godot` folder holds the import cache and the global class list. A new worktree has none,
and a merge can leave it stale. Then scripts fail with "Identifier not declared", "Could not
find type" or "Nonexistent function 'new' in base 'GDScript'", and some runs hang at low CPU.

1. Check that the project `.godot` folder exists. In a new worktree, run
   `godot --headless --path <project> --import` under a time limit before any test. Check its
   exit code.
2. Run the import again after each merge, branch switch, new `class_name`, and new image or
   audio file.
3. Run the import a second time when the first pass reports "no resource loaders". Scripts load
   before the new files import on the first pass.
4. Run tests, tools or the parse check only after the import ends.
5. Run the project parse check after every merge, including merges that touch only docs. It
   loads every `.gd`, `.tscn` and `.tres` under `res://` and prints `checked=N failed=M`.
6. To see the error of 1 failing script, run it alone with `godot --headless -s <script>` or
   with `--check-only`. Remember that check-only lacks autoloads, so read its autoload errors
   as noise.
7. When a test fails with "Nonexistent function 'new'", look for an undeclared class name in
   that script. Import and rerun before you debug.
8. When a test says an autoload is null, read the first `SCRIPT ERROR` line. The autoload
   itself failed to parse.
9. List untracked `.uid` and `.import` files after the import. Commit those of your own
   scripts and assets with the change that caused them. Leave the files of other work alone,
   because each branch makes different uids.
10. Run `git status` after an import. Restore tracked files that the import changed and that
    your change does not own, for example an audio bus layout. Commit or save your own edits
    in that file first, because `git checkout <file>` reverts them too.
11. If an untracked `.uid` blocks a merge, compare it with the incoming copy. Remove the local
    one and merge again. If 2 branches added the same `.uid` with different content, keep the
    side that owns the script.
12. If a run hangs at about 1 % CPU, check for a missing `.godot` folder first. Then stop only
    your own process.

## Isolated check project

Use this fallback when you must check your own scripts or resources, but the real project
cannot load because a file that other work owns is missing or broken. Do not edit that file.

1. Make a temp folder with `mktemp -d`.
2. Write a minimal `project.godot` there, or `rsync` the project folder into it.
3. Symlink your own asset folder into it, or copy the missing data file from the other
   branch.
4. Run a headless import in the temp project.
5. Run a `-s` check script that loads each of your resources and scenes. The parse check
   script works here.
6. Report the result as an isolated check. Run the check again in the real project after the
   other work merges.
