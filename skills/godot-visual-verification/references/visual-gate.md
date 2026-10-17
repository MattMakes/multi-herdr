# Windowed visual gate

Read this when you build, run or deflake the automatic windowed pass or fail gate.

A gate is the automatic pass or fail check. It does not replace step 8 of the method.

1. Build a gate mode in a demo scene. It draws a known pattern on a fixed path, plus untouched
   areas, with every real-time effect (wind, clouds, gusts) turned off.
2. Write a gate script. It runs each gate scene windowed on each renderer (for example Forward+
   and Compatibility), captures fixed frames and measures values in fixed regions.
3. Compare each value with a band. Print 1 `PASS` or `FAIL` line per renderer and scene, with
   the measured values.
4. Give each Godot run a time limit. On timeout, print a `HANG` line that names the step.
5. In the capture tool, add a readiness check that waits for a real render value, not only a
   frame count. Force a draw. Stop after a limit, for example 120 s.
6. Take the window lock around the whole gate, not around each run. Never run the gate headless.
7. Run the gate before you report done, and after each merge that touches shaders, materials,
   the palette or the main renderer. Keep every `PASS` and `FAIL` line, not only the tail.
8. If a value moves, run the gate on a detached base worktree. The base can show the same value,
   so the change is not the cause. Compare bytes between captures to rule out a stale frame.
9. If a check fails once, run the gate again and compare with known flake values.
10. When a change moves a region range on purpose, edit only the range lines. Add a new region
    only with the owner's approval.
11. For pixel fonts, check that each font is set only at its native size or a whole multiple,
    on whole pixels. A font made for 8 px at 7 px is blurry.

**Deflake a gate.** Count the flake rate with N runs in a loop. Write a probe that creates the
suspected condition (for example a window that covers the capture window) and compare it with a
control run. Apply the fix (window flags, readiness check, hang limit). Add a headless test that
fails without the fix, for example with an override of the Godot binary that forces a hang.
Prove the fix with 20 runs under the bad condition. Time the Godot run alone, not the lock wait.
