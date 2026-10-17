# Finish gate

Run this gate before a unit of work reports done or ready to land.

1. Merge the newest base branch. Resolve conflicts before any test; a suite on a file with
   conflict markers is not valid. When 2 units add rows at 1 place, keep both. Regenerate
   generated files.
2. Run the import and the parse check (see `godot-headless-runs`).
3. Run each design model or data check. Compare its FAIL lines with the base; your change adds
   none. Confirm the data exporter writes no change, or restore its output.
4. Run the full suite. Record passed, failed, skipped and any leak line.
5. Run the window gate on each renderer, if the project has one. Grep all PASS and FAIL lines,
   not only the tail.
6. Run `git diff --stat <merged base commit>`. It lists only your files. Review your own diff once.
7. Check that `git status` is clean, that staged renames are committed, and that none of your
   Godot processes still runs. Match the real binary path, not the word "godot".
8. Report exact counts, the commits and open items. Name each base failure and show that your
   diff does not touch it.
9. If the base moves during the gate, merge again and repeat steps 2 to 5.
