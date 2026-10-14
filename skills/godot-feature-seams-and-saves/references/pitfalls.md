# Pitfalls: the full table

This file holds every pitfall of `godot-feature-seams-and-saves`. SKILL.md keeps the 8 most important rows.

## Pitfalls

| Symptom | Cause | Fix |
| --- | --- | --- |
| The variant's special verb never fires in the encounter check. | No caller wires the verb. | Record the table as "before". Wire the verb. Run "after". |
| A test override fails after a seam change. | A new argument on a shared function; the override keeps the old signature. | Ask the owner. Update the override. |
| A rare result jump appears with the new variant. | A balance change merged by someone else. | Run the base type on the same build and the same seeds. |
| An upgrade node does nothing visible. | The node changes a number only. | Build a felt effect for every node. |
| `get()` returns null for a constant. | Constants are not properties. | Read the constant from the script, or expose a var. |
| A path follow drifts over time. | One large move per tick (for example 5 cm). | Use sub-steps. |
| A state check fails at random. | The check runs during a recovery window. | Wait for the idle state first. |
| 2 entities close the same window twice. | Each has its own local copy of the rule. | 1 shared seam that emits once per window. |
| A lookup crashes on a new id. | The id has no row. | Return an empty dictionary; fall back to the default. |
| A shared test breaks when a key is added. | The test copied a real row. | Remove the key from the test's copy only. |
| New art renders wrong in one view only. | A second render path ignores the new frame count or cell size. | Read frames and cell size from the row in every path. |
| Old rows fail validation. | A test requires the new field on all rows. | Require it only on the new rows. |
| A removed table still has references. | Old code names it in several places. | Run a parse check of all scripts; remove every reference. |
| A proto merge conflicts after upstream edits. | The proto edits the upstream file by hand. | A generator script applies the proto edits to any upstream copy. |
| The prototype costs tens of ms more than expected. | A debug hash runs every frame. | Put each debug cost behind an environment flag. |
| Mode tests break on each data change. | Tests use fixed distances or positions. | Read positions from the map API. |
| Positions are off by a constant. | 2 coordinate systems with their own origins. | Give the origin 1 owner. |
| Content of one mode shows in the other. | A hint or index list does not check the mode. | Filter by the row mode field. |
| A design doc names missing captures. | The report came before the captures. | Take the captures before the report. |
| An edited save loads without warning. | The encrypted file format has no integrity check against a key holder. | Sign with HMAC; compare in constant time. |
| A save decodes an object. | `str_to_var` decodes objects. | Save plain dictionaries only. |
| A load crashes on a missing number. | `float(null)` fails. | A number-or-zero helper. |
| A bad file prints an engine error. | The static JSON parse reports errors. | Use a `JSON` instance and check its error. |
| A test accepts a wrong type. | It compares with `== true`. | Check `typeof` first. |
| A tamper test does not change the file. | The test edits the wrong number form. | Match the exact serialized text, for example `2.0`. |
| A new reset copy becomes the oldest and is pruned. | It reused a deleted stamp in the same second. | Each new stamp is above the newest kept stamp. |
| Save tests fail in a sandbox. | The sandbox cannot write `user://`. | Use a disposable fixture under the test folder; list sandbox failures by name. |
| Old UI tests fail after a settings change. | They check the replaced boolean row. | Get approval, then update the tests of the replaced row. |
| Auto picks again on every launch. | The first-launch pick is not saved. | Save in the no-file branch. |
| UI tests break when a row is added. | They press "down" a fixed number of times. | Loop over the row count. |
| The editor rewrites the input map after the tool runs. | The tool saved typed arrays. | Convert events to plain arrays. |
