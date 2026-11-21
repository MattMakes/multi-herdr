# Progression scripts measure the game by its own code and players

The progression scripts under `skills/incremental-game-design/scripts/`
read balance from the game as it is built. They take the game's own
automated players and texts as evidence, and they follow the game's
choices instead of tuning their own.

## Consequences

- `pw_sim.Sim._payback_pick` reproduces the purchase choice of the
  measured game's own pacing buyer (`tests/pacing_buyer.gd` in the game):
  side-currency lines cheapest affordable first, then the main-currency
  line with the lowest wait + cost / value. It follows that GDScript buyer
  and is not tuned on its own.
- `scan_godot` `CatalogMap` keeps `all_texts` with the tests and tools
  folders, unlike the rest of the scan, because a Godot game's automated
  players (pacing buyers, reset rules) live there.
- The `pw_bench` inventory lists each string literal of a `.gd` file and
  each string property value of a `.tscn` or `.tres` file that has a digit,
  in the list `texts` of the bench file. Such a text can hold a copy of a
  balance number ("+25% pot value per level", "Unlocks at Shop Level 4")
  that the number ledger does not see and that would show the old value
  after tuning. A person accepts each one with a reason (the key `ok`), or
  builds the text from the data.
- The inventory leaves out strings that are clearly not player text
  (`TEXT_RULES`). Format-spec digits do not count ("%.2f", "%d / %d",
  "%02d:%02d", "{0} of {1}"). Code digits do not count: 1 to 4 capital
  letters and a number with no space ("FM-1 gap", "C12 Tend boost", "F2").
  "X2" is a factor, not a code.
- In `pw_bench.ROLE_RULES` the first matching rule gives the role of a
  balance number, so the order matters. The role None means "balance, and
  the name does not tell the role". `auto_unlock_at` is `auto_at`, not
  `unlock_at`. `prestige_reward_exponent` is `reset_bonus`, though
  `exponent` is a gain word. A base with a production word
  (`base_spores_per_sec`, `base_power`) is `base_income`, not a timer and
  not a growth.
- `pw_playlog.WINDOW_S` is 90 real seconds. The play log writes a
  heartbeat sample every 60 s, so the window before an event covers 1
  heartbeat and a margin, and never drops below 60 s.
- `pw_scaffold.WALL_SHADE` is 1 - 1e-5. Each ladder wall sits a little
  below the computed value, so float rounding of the requirement never
  lifts it above the value the player reaches at the target time.
