# The Lamp Road core is pure and step-size invariant

`skills/incremental-game-design/assets/godot/sample_game/core/game_core.gd`
is the math of the sample game Lamp Road. It has static functions only,
over a plain-data state (numbers, strings, lists and dictionaries). It
uses no node, autoload, signal, file, clock or engine random function. A
random value comes from the seed in the state. A function changes only the
state the caller gives and never changes `data`, which maps each data file
to its parsed content. This purity lets the tuning bench run the core
headless, on a thread and in replays with the same results.

## Consequences

- In `step` the lamplighter (the auto-buy) buys inside the step, at the
  time at which the light is enough. 1 long step gives the same purchases
  as many short steps.
- `max_step` returns 3600 s. The income is constant between 2 actions, so a
  long step is correct, and the session and the bench step in large
  chunks.
- The manual action of Lamp Road is a click: `active` gives light for the
  clicks of `seconds`. A game with a hold action uses the `hold_credit`
  pattern instead (`active` adds hold time, `step` uses
  `minf(hold_credit, dt)` of it at `hold_factor`). The session calls
  `active(dt)` then `step(dt)`, the bench calls `step` then
  `active(decision_s x active_share)`, so the bot's hold comes 1 decision
  late.
- `click.hold_power` in `data/economy.json` is the number for such a rule.
  Lamp Road does not have the rule, so that number is dead on purpose
  (README.md).
