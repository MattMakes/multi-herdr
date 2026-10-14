# Upgrade wing audit

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this to judge a group (a wing) of nodes: what each node really changes, which are dead, where cost walls are, and
whether the player can feel each node.

1. List the nodes with their stat and value from the tree source.
2. Search the game code for each stat key, including keys built at run time from a prefix. Record each reader. A
   stat with no reader is a dead node. Test your search on a known hit before you trust an empty result.
3. Flag a node as dead when its stat is already at a game cap at the run where the node opens.
4. From the pacing model or traces, list the run where each node opens and compare it with the boss runs. Compute
   the cost of each level in runs of income. Ask the owner to set the wall band before you read the costs. For
   example, the owner can call a level that costs more than 3 runs of income a wall. The right band depends on the
   run length and the genre, so do not use the sample value as a fact.
5. Agree a lean (wing-first) build with the design lead. Run it and the plain build at the same encounter states,
   each with its own save slot. Compare tries to kill and resources left.
6. Take 2 windowed runs at the same seed, with the wing at level 0 and at the max, and compare captures at the same
   time stamp. A still image does not show motion; capture a spin or a pulse as a clip.
7. Write 1 finding per node with no visible or felt effect. Propose cues (an arrow, a meter, a results line) as
   feedback findings, not number changes.
8. When a player says an upgrade does not do what its text says, find the code path. Make the effect match the text,
   and add a test from an existing fixture. Then search the bot and the balance tools that read the same stat.
