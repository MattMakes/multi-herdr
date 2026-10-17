# Design docs that match the data

Read this file when a design doc or a reference sheet states numbers that the model owns.

Design docs repeat ids, names, counts and number tables. Make a check script own that
agreement.

1. List the facts that more than 1 file states: ids, names, counts, order, gates.
2. Write a check script with 1 check per fact. It imports the model modules, parses the doc
   tables, and prints PASS or FAIL per rule.
3. Prove the check with a negative test. Copy the docs to a temp folder, change 3 facts, and
   confirm that the check finds all 3 (see `godot-test-loop`).
4. Save the "before" output. Fix 1 doc at a time and re-run the check after each doc until it
   prints 0 FAIL. Take numbers from the modules through the check's own functions; never type
   them by hand.
5. Generate doc tables from the model with a generator that replaces only its own marked block.
   Diff the doc and confirm that only table lines changed. Run it twice to prove it is
   idempotent.
6. Optional, for game text: first name the 1 source of truth for player-facing text, for
   example a localization resource (`.po` or `.csv` translation files) or a text table in the
   design docs. The other files must agree with it. If the source is a design-doc table,
   parse its rows into JSON with a count per section, and add a test that loads the JSON. If
   the source is a localization resource, check the docs against its keys instead.
7. For new content, write a model script that imports the current models, adds the new
   content, and prints its checks with ALL PASS. Check that the other models' outputs do not
   change. Generate the doc tables from it.
8. For a reference sheet, give each number its source: a model run in a temp copy, or a
   measured table read with `git show <branch>:<path>`. Remove every statement you cannot
   source.
9. Add lint rules to the check (style words, line length). Check new player-facing lines
   against the old maximum length. Add the check to the set every change runs.
