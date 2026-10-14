# UI review, layout tests and text fit

Read this when you review many UI states, turn a UI fault into a layout test, check text fit, or build UI before its spec lands.

## UI state capture review

Use this to build, review or declutter many UI screens and states without a person who clicks
through them.

1. Read the critique, the rubric and the UI spec.
2. Write a harness that takes `--state=<name>` and builds each state from a fixture made from
   real data, for example an empty, a partial and a full save.
3. Capture every state in 1 batch at the base size (1x) and at the window size. Take "before"
   captures with fixed names before you edit.
4. Zoom the tight areas. Make side-by-side images against the mockups. Check text contrast and
   off-palette pixels with a script.
5. Prove each suspected cause with an extra test state before you write the fix. Fix an overlap
   by margin and size, not by guess.
6. Write a test that every label fits its frame in the full state. Prove that the test can fail.
7. For a declutter request, write a headless tool that counts the words on each screen state.
   Count before. Shorten strings in the data source with exact-match edits. Check that only text
   fields changed. Remove repeated labels. Count after and report the drop in percent.
8. Write findings with a severity: blocker, major or minor. Text contrast near 1:1 is a blocker.
9. Take "after" captures with the same names and look at each one.

## UI layout invariant tests

A capture finds a defect once. A layout test keeps it fixed. State each rule as a geometric or
count check on real nodes.

1. List each defect from the review captures with its cause in the code.
2. State each rule as a check: 2 rects do not intersect, a label stays inside its parent, text
   width fits its box, a focused node stays inside the view, a sprite stays out of a HUD band.
3. Build 2 or 3 test states, for example early, mid and late game.
4. Write 1 test per defect or rule. Open the real scene. Await a settle helper that waits for
   layout and tweens to finish. Then compare global rects of real nodes. Check a label box against
   the full node frame, not only its centre.
5. Run each test and confirm that it fails for the reported reason.
6. Apply the smallest layout fix. When a test finds a real overlap, change the layout code, not
   the test.
7. Compute a limit from the real content (for example the free width from the longest real
   header). Do not tune a cap by trial.
8. Allow a small margin where the camera snaps to whole pixels.
9. Run the focused tests, then the UI test folders. Capture each affected state and look at it.

## Text fit check

Use this when you write story or UI text that must fit fixed boxes.

1. Find the limit of each line kind (toast, page, title, button) in characters or pixels.
2. Keep the lines in 1 source file, grouped by line kind.
3. Run a check that measures every line against its limit before the commit.
4. Fix each line over its limit, and run the check again.
5. In the scene, a layout test (UI layout invariant tests, above) checks the real rendered width. The character check
   catches most overruns earlier and cheaper.

`script-text-fit.md` holds `text_fit.py`; copy the block into a file. It checks a JSON file of line kinds, limits and lines. Use it as the pre-commit
check, then keep the in-scene width test for proportional fonts.

## UI function first

Use this when UI work and a UI design spec run at the same time.

1. Write event-driven tests for each screen: emit an event, check a label or a state.
2. Build each screen with the theme, the shipped art and a plain clean layout.
3. Do not wait for the spec. Report that the function is done.
4. When the spec lands, read its sections and mockups. Place nodes at the mockup coordinates and
   use the spec copy.
5. Update the tests to the new copy and run them. A text-fit test catches new copy that is too
   wide.
6. Capture a preview image of each screen and review it (SKILL.md method step 8).
