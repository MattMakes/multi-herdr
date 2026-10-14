# Bot balance loop: coach text from a winning bot

This file is part of `godot-bot-balance-loop`. Read it when players stall at a boss and need a hint taken from a winning fight.

When players stall at a boss, show the move that wins, taken from a winning fight and not from a guess.

1. Load a read-only copy of the stuck player's save, or build a state where the coached and tight lines both win.
   Prove with the real bot that the boss can be won at that state.
2. Run 1 verbose fight per winning line. Read when and where damage lands. Compare the traces before you name a line
   the winner; 2 lines can give the same rows for an unrelated reason.
3. Read the coach level rules first. A coach that a past kill turned off can not help this player.
4. Write the move per phase as a data row. Take the step and word limits from the card that shows the hint in
   your UI; for example, a card that fits 4 lines of 8 words gives at most 4 steps of 8 words. Make each step a thing
   the winning bot did, with the cue that starts each step (path size, timing cue, dodge line).
5. Propose the row and wait for approval. No encounter numbers change.
6. Show the row after N losses (for example 2) to the same boss. Test that it shows at the right loss count, only for
   that boss, and that the text fits its card. Capture the card in a winning fight.
7. Run the bot with the hint on. It must win at least as often as without.

A coach can not teach a fight that the resource always ends. Measure first.
