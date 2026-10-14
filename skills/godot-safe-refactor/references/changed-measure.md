# Find the cause of a changed measure

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

Use this when a measure today differs from a table that someone landed earlier.

1. Run your tool and the old tool on today's game, with the same state and the same seeds.
2. If the old tool also differs from the table, the game changed, not your tool. Extract the
   game at the landed commit: `git archive <hash> <project dir> | tar -x -C <temp dir>`.
3. Import that copy once. Run your tool on it. Check: it gives the landed table.
4. List the commits between the landed commit and today that touch the game.
5. Bisect: extract a middle commit and measure. Repeat until you find each change that moves
   the result.
6. Write each cause with its commit and its effect.

If a check uses a relative path, the scratch copy fails. Link the missing folder into the copy.
