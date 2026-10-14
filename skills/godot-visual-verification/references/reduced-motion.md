# Reduced-motion coverage

Read this when a change adds or touches a tween, flash, glow, shake, particle, growing effect or colour cue.

Use this when you add or change a tween, flash, glow, shake, particle or growing effect, or a
colour cue, in a game with a reduced-motion setting.

1. List every tween and moving effect that the change adds or touches.
2. Define the reduced-motion form of each: the end state at once, a fade, or a few still steps.
   Remove flashes.
3. Write the reduced-motion form into each new effect plan before you build it.
4. A timer effect (a growing warning) still needs to read as a timer. Grow it in 3 still steps.
5. Check each cue at a contrast of 3:1 or more, with a fill plus an outline.
6. Write a test that sets reduced motion, triggers the effect and checks the still form. Count
   active tweens to prove that nothing moves. Check that a settled shape is still there some
   seconds later.
7. Restore the settings defaults at the end of the test, so later tests do not inherit them.
8. Capture the reduced-motion form in a window too (SKILL.md method step 7).
