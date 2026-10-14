# Render and feel bug isolation

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

Use this when something draws wrong or draws nothing (a shader effect, a trail, an overlay),
or feels wrong (uneven steering, judder), and the cause is unknown.

1. List the candidate causes from the report.
2. Write a small probe outside the repository that sets up the case in the real project. Print
   the effect state, so you know the effect is on.
3. For a shader, back up the file. Route the suspect value into a strong visible output for 1
   run. If nothing shows, set that output to a constant. This splits "the data does not arrive"
   from "the output path is broken". Test each slot of an array uniform alone.
4. For a material that draws nothing, build 2 or 3 variants that each drop 1 feature (stencil,
   depth test, blend). Capture each. Keep the variant that draws.
5. Hide nodes or set a parameter to 0, 1 at a time. Count the changed pixels in a crop. Use a
   diff map between frames to find an effect that moves, because a small lift on bright pixels
   is hard to see by eye.
6. Repeat the captures on the second renderer if the game ships two.
7. For a feel bug, write a table tool that measures the isolated system per direction or case.
   Then write a tool that drives the real scene with the real input actions. Compare world
   values with screen values. Dump the internal state that each suspicion depends on, for
   example a mask image.
8. For a leak line at exit, check that the base commit has none. Bisect commits, then single
   changes. Run the leaking test with `--verbose` to list the leaked objects. See
   `godot-test-loop` for the full leak hunt.
9. Test the fix hint in the probe before you send it. Send the cause, the numbers and the
   tested hint to the owner, or ask for the decision, before you change shared code.
10. Take a reference before the fix (see the Method). Fix with regression tests. Run the table
    tool again. Restore every debug edit.
