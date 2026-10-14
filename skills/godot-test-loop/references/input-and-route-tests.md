# Input injection and route trace tests

## Input injection

Drive menus and the player through the same input actions a real player uses.

1. Use only `ui_*` actions in menu code, so keys, pads and tests share 1 path.
2. In a test, press the action, step 1 or more frames, then release it (`tap()` in
   `test_case.gd`, see [script-test-case.md](script-test-case.md)).
3. Step 1 more frame after a release before the next press. Why: without it the code sees no new
   press edge, and the second tap is lost.
4. For captures and bot runs, reuse the steer code that presses the real move actions.
5. Restore global input state (device family, glyph set, focus) in `after_each`, so a failed test
   does not change later tests.
6. The default `ui_accept` and `ui_cancel` have no joypad events. Let 1 owner change the input
   map; everyone else tests through action injection.

## Route trace regression test

Use this when a player reports that a screen shows the wrong state and several menu routes lead to
that screen.

1. List every route to the screen.
2. Write 1 test with 1 case per route. Each case prints 1 fixed-prefix log line with the route
   state and the shown state.
3. Run it and save the log lines as evidence.
4. Decide with the owner whether the result is a bug or the design.
5. Add cases for routes that change the state with no player choice: focus on open, hover from a
   resting pointer, restored selection. Fix each one test-first.
6. Keep the test as a regression test.

A known trap: a hover signal fires from a pointer that rests over a card when the screen opens.
Take focus from the mouse only on real mouse motion.
