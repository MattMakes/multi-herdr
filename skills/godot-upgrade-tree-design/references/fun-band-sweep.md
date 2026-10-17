# Fun-band config sweep

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when global tuning (price base, caps, curves, capstone spikes, stage multipliers) must be chosen by a stated
fun target with a panel of bot player types.

1. Move the tuning knobs into a small config file that the dev mode reads. Add a new knob to the tree code with tests
   first. Update pinned tests in the same change as the generator.
2. Write the fun band with the owner: tries per encounter kind and minutes per chapter, for example "not won on the
   first try, not lost 10 times". The score is the share of (player type, encounter) pairs in band. Add worst-case
   measures: the worst pair, pairs over 10 tries, first-try wins.
3. Write 1 config file per candidate. Round A plays all configs x all panel types x 3 seeds, in the background at low
   priority.
4. Watch progress by the count of finished result files, not by log lines. Grep every log for script errors; a run
   with an error is invalid.
5. Rank configs by the score. Leave out an encounter that every config passes on try 1, with the owner's approval.
6. Run more rounds that move 1 knob at a time. When a target changes, rescore the saved results; do not replay.
7. Play the 2 or 3 finalists and their base on 6 seeds. Show each pair as mean (min to max).
8. Recompute each claim in your notes from the result data before you report. Write a recommendation; the owner
   picks. Set the chosen values and run the full test suite.

A difference of 2 pairs in 30 is inside seed noise. A chapter that every config finishes too fast is a design
finding, not a tuning fault. If price alone cannot set a chapter's pace, stop the price grid and report it.
