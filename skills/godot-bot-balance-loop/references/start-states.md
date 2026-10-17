# Bot balance loop: start states from the model

This file is part of `godot-bot-balance-loop`. Read it before any fight measure. It builds the meeting state from the balance model or a real bot run.

A fight measured at a hand-set level map tells you nothing about a real player. Start every measure from a state a
real player can reach.

1. Read the pacing model's ladder and spend code. Reuse its functions; do not write a second copy of the price rule.
2. Run the model with a fixed buy rule, or replay the real buys of a full bot run. Record the run index of each
   first meeting.
3. Write a states script that prints 1 JSON state per case: variant, upgrade levels, passives, resources and meeting
   run. Import the model from a scratch copy, so the run writes no export.
4. Stop income to a variant when a newer variant unlocks. Spend left-over budget on nodes that are new in the tree.
   Order every set by a fixed list, so the output is the same on each run.
5. Check that every case gives the same meeting run as the landed model, and that derived stats match the game.
6. Point every fight tool at the states file. Make the states again after any change to the economy, the tree or a
   meeting time.
7. To find the first winning point, make 1 state per run index k after the meeting. Sweep k coarsely (2, 4, 8),
   then fill in between, with a threshold such as 6 of 10 wins.
8. When a later state wins less than an earlier one, bisect the middle states, then single buys. Find the 1 level
   that breaks the fight, and compare a win row with a loss row to explain it.
9. For a "what does this player need" question, load a copy of the player's save, never the real file. Spend the
   bank with a greedy cheapest-first buyer through the game's own buy API. Add +1, +2, +3 and +6 runs of model
   income, and report the first state that meets the stop rule.

Mark a state where the run ends before the encounter rises as "no fight", apart from a loss.
