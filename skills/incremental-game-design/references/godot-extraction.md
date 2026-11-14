# How to read the progression out of a Godot project

The goal is a model (model-schema.md) that matches what the build does, not what the design
document says. Where the two differ, the difference is a finding.

## 1. Scan

```bash
python3 scripts/scan_godot.py <project_dir> --out <work_dir>/scan.json
```

The scan finds `project.godot` (also in a subfolder; if there are several, it prints all and
the one it chose). It skips `addons`, `tmp`, `out`, `logs`, `ai_docs`, build folders, and
nested projects. The report on stdout is short. `scan.json` holds the full lists.

- **Economy signal.** One verdict line: strong, medium, or weak, from three counts: lines
  with `pow(` and a cost word, data files that have both a cost key and a growth key, and
  unlock words. **Weak means stop.** The project may not be an upgrade economy (a platformer
  with a level select, a puzzle game). Do not force a progression model onto it. Tell the
  user what the game has in place of an economy, and review that part only.
- **Autoloads.** Global state lives here. Names such as `Data`, `Meta`, `GameState`,
  `Economy`, `Save`, `Events`, and `Telemetry` are the first files to read.
- **Existing balance tooling.** Files and folders named for a simulator, diagnostics,
  balance tests, tuning targets, or autoplay policies. Open them before you model: a project
  that has its own simulator has a better source of truth than a new model (part 4).
- **Data files with economy keys.** JSON, `.tres`, `.cfg`, and CSV files with keys such as
  `cost`, `growth`, `mult`, `unlock`, `tier`. Keys match as whole words, so `scale_w` and
  `export_filter` do not count. `[cost + growth]` marks an upgrade table. For the top files
  the report describes each top-level key: name, type, size, and item keys.
- **Scripts with economy math.** GDScript and C# lines that contain `pow(`, `**`, `log(`, or
  `sqrt(` and an economy word. These lines are the cost and scale formulas.
- **Scene overrides.** Economy properties that a `.tscn` file sets in place of the script
  default. Then one line each for **save, unlock, offline, and telemetry** code.
- **Beat hooks.** The signals whose names carry an economy or unlock word, where each is
  emitted, the handler functions, and the feedback found in each handler (part 5).
- **Feedback code** (script hits and scene node types) and the **feel checks** (part 5).

Files in `tests/` and `tools/` come last in each list. The scan ranks. It does not
understand. Each hit is a lead: read the file before you state it as a fact.

## 2. Read in this order

1. The data files with the most economy hits. Note the shape: a list of nodes with `cost` and
   `growth`, a table of tiers, a list of enemies with HP multipliers.
2. The function that turns data into a price. Search for the formula:
   `grep -rn "pow(" --include="*.gd" <dir> | grep -i "cost\|price\|growth"`.
   A typical line is `cost * pow(growth, level)`. Confirm the formula type (`exp`, `linear`,
   `poly`) from the code, not from the key names.
3. The function that turns upgrades into power. Find where multipliers combine. Look for
   `+=` on a bonus (additive pool) against `*=` (multiplicative). This sets `pct` against
   `mult` in the model.
4. The gates. Search for the unlock vocabulary from the scan (`unlock`, `req_level`,
   `requires`, `is_unlocked`). Each gate becomes a requirement.
5. The reset. Search for `prestige`, `rebirth`, `ascend`, `reset`, `new_run`. Find what the
   reset clears and what the save keeps. For a run-based game, see the note below.
6. The save file writer. The saved fields are the list of everything permanent.

For a large project, do steps 1 to 5 one at a time and write the formulas down with file and
line before you write the model. In a fleet, ask the orchestrator for a second worker if the
project is too large for one pass. Keep the model writing for yourself.

**Run-based games (roguelites).** No code says `prestige`: each run is the reset. Find
these four things, then model one run as the model's `prestige` block.

- The end of a run: a signal such as `run_ended` or `game_over` (the beat hooks table lists
  it as kind "reset"), or the results screen.
- The run reward: what the end-of-run code adds to the save. Search the handler and the
  results screen for `+=` on a saved currency. The formula there (distance, kills, haul) is
  the prestige gain formula.
- The meta spend: the screen between runs (shop, tree, shed) and the data file behind it,
  usually the top `[cost + growth]` file.
- What a run keeps and loses: compare the fields that the save writer stores with the
  fields that the start of a run sets again.

Count pacing in runs: how many runs to the next meta purchase and to the next unlock.

## 3. Godot specifics

- **Resources (`.tres`, `.res`).** A custom `Resource` class with `@export` fields is the
  usual home of upgrade definitions. Text `.tres` files are readable. The class script
  (`class_name UpgradeData`) gives the field meanings.
- **`@export` on scene nodes.** Balance numbers can sit in `.tscn` files as property
  overrides. Search the scenes for the property name when the script default looks like a
  placeholder.
- **Signals.** Unlock moments usually emit a signal (`unlocked`, `level_up`,
  `prestige_done`), often on a bus autoload such as `Events`. The handlers are where the
  feedback is, or is not. The scan's beat hooks table lists them.
- **Big numbers.** GDScript `float` is a 64-bit double: it holds values up to about 1.8e308
  and has 15 to 16 significant digits. `int` is 64-bit and overflows near 9.2e18. A game
  that stores a currency as `int` has a hard ceiling: report it if the simulation passes it.
- **Timers.** `_process(delta)` income, `Timer` nodes, and `Time.get_unix_time_from_system()`
  for offline progress. A system on real time is a second clock: mark it `"clock": "real"`.
- **Tests and tools.** A project with a run simulator or balance tests already has a source
  of truth (the scan lists it). Use its numbers and its target file to check the model.

## 4. Check the model against the build

Before you trust the workbook, compare one or two facts with the game:

- The price of the fifth level of one upgrade in the model against the code formula.
- The time to the first unlock against a real play session, a recorded run, or telemetry.

If the project logs events (the scan lists telemetry code), ask for a log or find one in the
project. Real unlock times beat simulated times. Put the real times in the report beside the
simulated times and state the ratio: that ratio is the "human factor" for this game, and it
calibrates each later run of the simulation.

Godot can run a project with no window: `godot --headless --path <project_dir> --script
<res://path/to/tool.gd>`. If the project has its own simulation or diagnostic tool, prefer
it. Run project code only when the user asks for it or confirms it: a tool script can write
to the save file.

## 5. Juice audit

Balance decides when a beat happens. Juice decides if the player notices. A verdict on juice
needs a file and a line. With none, write "not audited".

### Method

1. Start from the **beat hooks** table of the scan (`beat_hooks` in `scan.json`). Each row
   is one signal: its kind (purchase, unlock, ladder step, level up, milestone, reward,
   reset), where it is emitted, the handler functions, and the feedback found and missing.
   The categories are tween, particles, sound, shake, popup/toast, animation, hit stop, and
   haptics. A `*` marks feedback found in a function that the handler calls.
2. Match each beat kind of the Timeline to a row. A beat kind with no row has no signal:
   find the function that performs it (`buy`, `unlock`, `prestige`) and read what it calls.
3. Open each handler and confirm. The scan matches words. It misses feedback that a direct
   call starts, an `AnimationPlayer` track, or a shader. It also counts a tween that only
   moves a layout. Correct the row from what you read.
4. Compare the rows with each other and with the table below. **A system unlock or a reset
   with the same feedback as a purchase is a finding**: the workbook gives it weight 5 and
   the game gives it weight 1. A handler that only sets a label text is "no feedback".
5. Write the audit as a table: beat kind, signal, handler (file and function), feedback
   found, feedback missing, and the fix.

| Beat | Minimum feedback | Strong feedback |
| --- | --- | --- |
| Purchase | Sound, number change | Button punch (scale tween), cost count-down, the bought thing visibly changes |
| First purchase of a line | The above, plus a short highlight of what is new | A one-line explanation at the moment it matters |
| Ladder step | Banner or toast, reward count-up | A change in the world: new colour, new enemy, music layer |
| Feature unlock | A new button appears with an attention effect | The game pauses for a beat, shows the feature, and gives a free first use |
| System unlock | Full-screen or large panel moment, distinct sound | A reveal that was teased earlier (a locked tab, a silhouette), plus a guided first action |
| Prestige or end of run | A screen transition, a count-up of the gain | The first minutes of the new run are visibly faster: show the old and the new speed |

### Feel checks of the genre

The scan prints hits (file and line) or "none found" for each. Both are leads: search once
by hand before you report a gap.

| Check | Good looks like | Search for |
| --- | --- | --- |
| Large-number formatting | One function formats every number. Suffixes or scientific notation start before a number is too wide for its label, and three or four significant digits stay visible | `format`, `short_`, `suffix`, `"K"`, `snapped` |
| Affordability cue | A button changes state at the moment the player can pay (colour, glow, sound), not only on a click | `disabled =`, `modulate`, `can_afford` |
| Locked or teased content | The next system shows before it opens: a locked tab, a silhouette, a "???" row with its requirement | `locked`, `silhouette`, `"???"` |
| "Earned while away" screen | On return the game shows what the player earned and for how long, then counts it into the total. Needed only if offline progress exists | `offline`, `welcome_back` |
| Count-up on rewards | The reset or end-of-run reward counts up with a tick sound, and big gains take longer | `tween_method`, `count_up` |
| Stat or save export | The player or a tool can read the state: a stats screen, a save export, or a run log | `JSON.stringify`, `clipboard_set` |

Godot tools for the fixes: `create_tween()` for punch and count-up, `GPUParticles2D` or
`CPUParticles2D` for bursts, `AudioStreamPlayer` with pitch variation for repeated sounds,
`Engine.time_scale` for a short hit stop, a `CanvasLayer` for banners, and `AnimationPlayer`
for staged reveals.
