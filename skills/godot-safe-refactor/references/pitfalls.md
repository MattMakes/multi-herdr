# Pitfalls: the full table

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Two identical runs of a fight check give different results | The check is not deterministic | Use a deterministic trace as the proof, not the fight check |
| Two base runs give, for example, 90 and 96 hits | Frame timing noise | Fix the frame rate, turn off delta smoothing, run the base twice to measure the spread |
| The seed differs after a change | A new start rule uses the random generator | Diff the telemetry to the first event that differs |
| A rewrite with cached trig changes scores slightly | Float precision | Reject it; keep only changes that are identical and win in paired timing |
| A later run is faster in unchanged code | Machine load drift | Report only back-to-back pairs |
| The identity proof has no kill events | The scenario kills nothing | Add a fixed enemy to each case |
| A golden compare fails on float rounding | The reference holds floats | Compare with a stated tolerance where the reference is float |
| A batching switch changes a few pixels in some pairs | Draw order or precision | Report the exact pixel count and decide; do not call it identical |
| Tests outside your file list fail after a rename | They call the old name through `call()` | Search string calls first; keep private forwarders |
| A rename breaks the parse | The new name already exists in that scope | Run the parse check; pick a new name |
| The suite fails after a move | A test calls a moved private function | Search the tests for every moved name before the full suite |
| A capture diff appears in a "pure cleanup" commit | The cleanup changed a draw argument | Capture again before that commit; keep cleanups in their own commits |
| `git grep -E '\bname\b'` finds nothing | `\b` is not portable | Use `git grep -w` |
| An image diff script fails to start | The imaging library is not in the default Python | Use another Python, or ImageMagick `compare` |
| Your new tool shows a gap against an old table | A later game change, not the tool | Run the old tool on today's game first |
| A probe run with `-s` fails to compile | It names class names before autoloads exist | Load game scripts at run time with `load()`; see `godot-headless-runs` |
| A dead-code search misses callers | It covers the game folder only | Search the whole repository, including tools |
| A rule fix leaves drift | The rule has several copies (logic, view, demo, tool) | Map all copies first; move the rule to 1 helper |
| A parity test passes for the wrong reason | The fixture shuts 2 rules at once | Shut only the rule under test |
| A test outside scope names a removed constant | The constant moved | Ask for a grant; make the test read values from data |
| Headless runs show no parser warnings | The runtime does not print them | Read them from the language server |
| `override.cfg` does not turn on warnings | The editor ignores it for warnings | Edit `project.godot` in a scratch copy |
| A "dead" function is live | A string-built call reaches it | Search string calls and built names first |
| The layout scan shows many cycles | Matches in comments | Verify each hit in code |
| The warning check prints every kind | No filter | Filter to the kinds the plan names |
| A review cites a file that is not in the tree | The tree moved during the review | Review at a named commit |
| A citation points at the wrong line | No check pass | Check every citation before you report |
| Component tests pass, but the game is not playable | The main scene is a placeholder | Rank the integration gap as its own finding |
| A fix for 1 finding counts a case twice | The test did not pin the exact count | Report to the owner at once; assert the exact count |
| A copy change breaks an assertion in another unit's file | Scope | Report blocked; wait for the grant |
| A shader push does not show | A helper (for example a distance function) returns 0 | Test the helper alone through a visible output |
| A light lift is invisible in bright areas | Most pixels are already bright | Judge with a diff map |
| A palette snap maps a tint to an unexpected colour | Nearest-colour search | Compute where each colour lands |
| A hash gives negative angles | `%` keeps the sign | Use `posmod` |
| Steering drifts off the diagonal | A short key-release gap between ticks | Add a short release grace with a test |
| The first suspect is wrong | A guess, not a measure | Dump the internal state the suspicion depends on |
| A fix hint uses an API that does not exist | The hint was not tested | Test each hint in the probe first |
| A stencil pass draws nothing | Renderer limits | Try a variant, for example an inverted depth-test silhouette |
| A leak line at exit after a refactor | A static function casts to its own class and makes a script cycle | Use untyped access |
