# Design research

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when a design question or a large redesign request needs evidence and options before the owner decides.

1. Frame the question. Read the current design, data and base stats; map the relevant code with read-only searches.
2. Check a redesign request against the stated fun targets and earlier owner decisions. Name each conflict or
   reversal with its date, and offer fixes.
3. Search about 10 or more shipped games, 1 search per game or concept, in parallel. Fetch each page that holds a
   claim and quote only what it says. A claim from a search snippet only is not verified.
4. Name each pattern and apply it to the game's own classes and levels. Mark sourced facts apart from inference, and
   mark forums and wikis as opinion.
5. For a fight that numbers cannot fix, read its measured tables and its script. Write 1 measured cause per fight,
   then 1 or more worked mechanic options per cause. "None" is not an option.
6. Write a research note with patterns, sources and fit. Add 1 numbered decision item with options and a
   recommendation (format in the next section). Keep the plan in the repo, not in a private agent folder.

## Gameplay changes are the owner's call

Number tuning, behaviour-neutral bug fixes and perf fixes can go ahead. A new or removed mechanic, verb, behaviour or
cue is a proposal, not a build. A help or coach text that changes how players fight counts as gameplay.

- Write each proposal as a numbered item in 1 decision file: the facts, the evidence (tables, traces, captures), 2 to
  4 options, and 1 recommendation.
- Look for an option that is a new number on an existing mechanic. Prefer the local lever (the entity's own data row)
  over a new global mechanic; a global mechanic for 1 boss is an over-reaction.
- Record the answer under the item in the owner's exact words. Only the owner's own words are direction.

The ask-and-wait protocol for a team of agents is in `godot-fleet-game-dev`.
