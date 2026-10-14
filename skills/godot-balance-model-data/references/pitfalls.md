# Pitfalls: the full table

SKILL.md lists the most important rows. This file holds every row.

| Symptom | Cause | Fix |
| --- | --- | --- |
| A report-only model run left tracked exports changed, or re-exported another change's rows. | Modules write exports as a side effect. | Run on a scratch copy, or `git checkout` the files after each run. Check first whether a script writes files. |
| A scripted replace changed 2 rows, or silently matched 0. | Replace by value or by line number. | Replace by unique text and assert exactly 1 match. |
| A plain re-run on a new base changed many rows you did not touch. | The checked-in JSON already differed from the model inputs. | Stop and report the drift. Ship only your rows. |
| A base run in a temp folder showed 0 FAIL lines. | The copy missed sibling files that the model imports. | Archive every folder the model opens. |
| New FAIL lines were invisible; a module exited 1 anyway. | Pre-existing FAIL lines. | Compare FAIL key sets. The rule is "the same FAIL keys before and after". |
| A derived value moved when its source moved (for example health from appear time). | A hidden derivation. | Read the derivation and move both values on purpose. |
| Folder-only tests passed; other tests broke later. | A stale parity fixture, a new signal with no subscriber, fixtures that pin old rows. | Run 1 full suite after any data, price or signal change. Plan "regenerate the parity fixture" as a step. |
| Keys added to a shared stats dictionary broke the parity key count. | The stats function changed shape. | Add a separate apply step. Keep the stats function unchanged. |
| The game read old values for one row or one price family. | The exporter kept a frozen copy, or skipped a family. | Copy every field from the source row. Test that the 2 copies agree. Read the JSON diff. |
| Every existing row gained a new default field. | The exporter writes defaults. | Write the field only on rows that need it. |
| A placeholder row broke row-wide tests or added an unwanted trait. | It was copied from a real row. | Give placeholders neutral values and a `placeholder: true` flag that row-wide tests skip. |
| Removing a retired row broke many tests. | Tests use it as a fixture. | Keep the row. Remove it in a separate cleanup. |
| Fixture or export rows were missing. | An imported module filtered a shared table on import. | Deep-copy the table before the import and restore it after. |
| A second model gave different results from the same seed. | It replays a fixed draw order and a new factor took a draw. | Use the expected value of the new factor, not a new draw. |
| A doc table tool cut sections, or changed the doc on each run. | The tool is not block-scoped or not idempotent. | Replace only a marked block. Restore with `git show HEAD:<file>`. |
| Sorting failed in Python 3. | Mixed key types in tuples. | Sort with `key=str`. |
| Moving nodes between lists moved other nodes. | Layout derived from list order. | Keep the old layout slots. |
| A slow generator looked stuck. | It runs a full simulation. | Send output to a file. Check the process list before you call it stuck. |
| The model passed and the game failed. | The model read its own copy of the rows. | Make the model read the game rows exactly. |
| Measured power was a fraction of the model value at the first meeting. | The model was never calibrated. | Measure the real game first, then calibrate. |
| A fit passed every target with absurd values. | No guard rule. | Add a band rule, for example 0.5x to 4x of a reference price. |
| One shared lever could not satisfy 2 opposite targets. | Over-shared lever. | Use 1 value per currency, with a fallback per variant and a written reason. |
| A regenerate made no change. | The fixture was not the cause. | Stop and find the real cause before you change anything else. |
| The parity test showed a double count. | Stats applied a legacy list and the new rows. | Count each node once. |
| A lever did not move the output. | Another limit binds (a time cap, a pay cap). | Test that each lever moves the output before a grid. |
| The bot earned none of a rare currency, so the tree dead-ended. | A gate needs a currency that real play does not give. | Change the gate so a tier needs only the earlier tier. |
| NumPy was not installed. | Standard library only. | Solve a small least-squares fit with Gauss elimination. |
| Parallel model runs crashed or mixed results. | They wrote the same output file. | Give each run its own output copy and its own model state. |
| A model tool assumed the wrong run end. | A hard-coded end. | Read the end from the data. |
| A search grew too large and printed nothing. | Too many levers. | Stop, narrow the lever set, restart from the best near-fit. |
| A background what-if changed tracked files during a commit. | It writes outputs. | Run in the foreground, or stop and restore first. |
| A per-entity random seed pushed values across a band edge. | Small margins. | Check margins before the change. Apply the seed in every module that compares the same numbers. |
| The model ladder hid a wall that real players meet. | Model states were a near-maxed player. | Build states from a full-run purchase log. |
| Pacing ran past the end of the ladder data. | Too few states. | Pad with the last state and spend nothing on padded runs. |
| The shell split a multi-edit override. | Quotes. | Quote each override as its own argument. |
| Logs grew to megabytes. | Per-second lines. | Keep only the summary lines the tools read. |
| A new rule arrived mid-task. | Changed requirements. | Record it first, replace every out-of-band value, then fit again. |
| A count check counted 1 item twice. | 2 docs list it. | Remove duplicates before a count. |
| New player-facing lines overflowed the UI. | No length check. | Check against the old maximum length. |
| `sed` stopped early or failed on macOS. | A loose range pattern; BSD `sed` syntax. | Use a tighter pattern, an edit tool, or `sed -i ''`. |
