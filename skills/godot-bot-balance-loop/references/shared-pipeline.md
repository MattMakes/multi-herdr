# Bot balance loop: one shared measure pipeline

This file is part of `godot-bot-balance-loop`. Read it when many workers or many per-unit tools must give tables that compare.

When many workers measure many cases (encounter x mode x variant), or many per-unit tools must measure 1 joined
branch, give them 1 pipeline so the tables compare.

1. Work from the integration branch. List each unit's fight tool and states script.
2. Build the states file from the model on that branch.
3. Write a generator that copies each tool and applies the shared patch (the shared skill draw, case seams, a guard
   that the right encounter rises) by exact text replacement. Each replacement asserts exactly 1 match.
4. Write 1 entry script: 1 encounter, 1 mode, 1 steering, a log tag, a seed list, 1 headless call, through the slot
   runner with `nice` and an alarm.
5. Reproduce 1 landed table with the pipeline before any fan-out. If it differs, find the cause first.
6. Run 1 smoke case per tool. Check exit code 0 and no script error lines.
7. Write 1 table script with the shared rules as columns (rescued kills as losses, coached against tight).
8. Fan out by variant. Each worker smokes first, runs steerings in seed chunks under the tool time limit, writes
   tables and notes in its own folder, and leaves game and design files clean.
9. Run a parity check on a few seeds: headless, then in a window at the real frame rate. Keep the headless table as
   the measure; the window run is a capture.
10. Commit the tables and send the FAIL rows to the data owner.

Grep each tool for the fields it logs before the patch names them.
