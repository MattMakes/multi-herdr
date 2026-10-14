# Find every enforcement site

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

Use this before you delete, move or rename a name, or before you change a rule or a mechanic.
A rule that lives in several places drifts when you fix only 1 copy.

1. List every name you delete or move. List every key and gate field of the rule you change.
2. Search the whole repository, not only the game folder. Use whole-word matching:
   `git grep -w <name>` or `grep -rw <name> .`. Do not use `\b` with `git grep -E`, because
   some platform builds give no hits for it.
3. Also search for string calls: `"<name>"` near `.call(`, `get(`, `set(`, `has_method(`,
   `connect(`. Also search for built names, for example `"_on_" + kind`.
4. Test the search pattern on a name that you know exists. An empty result from a broken
   pattern looks like proof that the name is unused.
5. Write each enforcing path in your notes with the file and the function. Note how each copy
   differs from the others.
6. Keep every name that a tool calls. For a moved constant, keep an alias in the old script.
7. For a rule that a view and the logic both decide (open, locked, buyable), move the rule to 1
   helper on the logic side. Make the view, any demo copy and the tools call that helper.
8. Write a parity test. It walks every item over real save snapshots (fresh, mid-game, full) and
   asserts that the view state equals the logic answer. Check: the test fails on the old view.
9. For a removed or changed mechanic, write failing tests for the new behaviour first. Then
   change or remove each listed path.
10. Run a parse check over every script. It finds stale references that the search missed. Ask
    the owner before you edit a stale test outside your scope.

Set test fixtures so that only the rule under test is shut. A fixture that shuts 2 rules at
once passes for the wrong reason.
