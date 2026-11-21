# Play data: the play log and its analysis

`scripts/pw_playlog.py` reads a folder of play logs. It relates the "felt good" marks, the "dragged" marks and the quit points to the live fun values, and proposes band edges for the fun sheet (`references/fun-analysis.md`). The research is in `docs/research/fun/tuning-controls-and-play-data.md` of the lab repository (section 6, the play data).

## Contents

1. The contract
2. Common fields
3. Record types
4. The fun values
5. Command line
6. What the script computes
7. Limits

## The contract

The play log format is a contract. Any game can write it, and the script reads it with no knowledge of the game. The game writes 1 file for each session, in JSON Lines: UTF-8, 1 JSON object on each line. The file name is `<sid>.jsonl`. The game flushes the file at each heartbeat and at each mark, so that a crash loses at most 1 line.

The script needs only these parts of the contract:

1. The fields `e`, `rt` and `sid` on each line.
2. The records `session_start`, `heartbeat`, `mark` and `session_end`.
3. The object `fun` on each `heartbeat` and each `mark`, with the live fun values as numbers or `null`.

The other fields and records are optional for the script. A game can add fields. The script ignores a field and a record type that it does not know. Each line must be a JSON object with a field `e`. A line that is not one stops the script with the file and the line number.

The log holds no user name, no account id, no machine name, no file path, no time of day and no free text from a player. `sid` is random for each session. `save_id` is random for each save.

## Common fields

Each line has these fields. The reference game writes all of them. The script reads `e`, `rt`, `sid`, `t`, `stage` and `set`.

| Key | Meaning |
| --- | --- |
| `t` | game seconds since the session start (real time x the time scale) |
| `rt` | real seconds since the session start |
| `e` | the record type |
| `sid` | the random session id |
| `stage` | the progress stage of the game |
| `spores_log10` | log10 of the wallet of the main currency, or `null` at 0 |
| `income_log10` | log10 of the income of the main currency each second |
| `run_s` | seconds since the last reset |
| `prestiges` | the count of resets |
| `scale` | the time scale |
| `set` | the name of the tuning setting in force: a saved name, `base` or `custom` |
| `save_id` | the random id of the save file |

## Record types

Each example is 1 line. The examples show the extra fields only, after `...` for the common fields.

`session_start`: the game starts or loads. `controls` holds the factor k of each tuning control.

```json
{"t": 0.0, "rt": 0.0, "e": "session_start", "sid": "5f6e497832810bee", ..., "format": 1, "build": "dev", "data_hash": "c7dbcbef5ec506470f2592efbde2e4ee", "save_state": "new", "day": "2026-11-15", "controls": {"c1": 1.0, "c2": 1.0}}
```

`heartbeat`: each 60 real seconds. It holds the live fun values and the controls.

```json
{"t": 60.0, "rt": 60.0, "e": "heartbeat", ..., "hand_buys_60s": 4, "auto_buys_60s": 0, "tends_60s": 0, "income_growth_60s": 0.41, "auto_buy_summary": {}, "fun": {"fm1_now": 22.0, "fm2_share": 0.0}, "controls": {"c1": 1.0}}
```

`mark`: the player presses a mark key. `valence` is `good` (F7, "this felt good"), `drag` (F8, "this dragged") or `note` (F9, a developer note with `text`).

```json
{"t": 2301.0, "rt": 2301.0, "e": "mark", ..., "valence": "good", "fun": {"fm1_now": 583.0, "fm2_share": 0.0}}
```

`purchase`: a purchase by hand. `wait_s` is the time since the purchase became affordable. `gain_pct` is the income change in percent.

```json
{"t": 2302.0, "e": "purchase", ..., "id": "line_4", "level": 4, "count": 1, "price_log10": 4.243, "wait_s": 0.0, "gain_pct": 21.0}
```

`unlock`: a purchase line, a feature or a tier opens.

```json
{"t": 1717.0, "e": "unlock", ..., "id": "line_4", "kind": "line"}
```

`stage`: a stage clears. `gap_s` is the time since the stage before.

```json
{"t": 2302.0, "e": "stage", ..., "reward_log10": 2.419, "gap_s": 584.0}
```

`milestone`: a collectable or a milestone with an id.

```json
{"t": 900.0, "e": "milestone", ..., "id": "collect_3"}
```

`reset`: a prestige reset.

```json
{"t": 3100.0, "e": "reset", ..., "gain": 3.0, "run_s": 1450.0, "stage_at_reset": 9}
```

`input_burst`: 3 or more inputs inside 5 s. 1 line when the burst ends.

```json
{"t": 1724.0, "e": "input_burst", ..., "inputs": 3, "length_s": 0.0, "kinds": {"buy": 3}}
```

`idle`: the input stops for the active window of the game (`state` is `start`), and the input comes back (`state` is `end`).

```json
{"t": 2312.0, "e": "idle", ..., "state": "start", "length_s": 10.0}
```

`control_change`: a tuning control moves and stays for 2 s.

```json
{"t": 600.0, "e": "control_change", ..., "control": "c3", "from": 1.0, "to": 1.25}
```

`setting_load`: a saved tuning setting loads.

```json
{"t": 30.0, "e": "setting_load", ..., "name": "fast start"}
```

`session_end`: the session ends. The quit point is the line before it. When the game stopped with no `session_end`, the next start adds one with `"recovered": true`, `active_s` `null` and `length_s` from the last line.

```json
{"t": 2400.0, "rt": 2400.0, "e": "session_end", ..., "length_s": 2400.0, "active_s": 152.0}
```

## The fun values

The object `fun` holds the live values of the fun meter of the game. A value with no data is `null`. `out` lists the rows outside their band in the game, and the script does not read it. The script knows the keys below. It reports an unknown key with its key as its name and no unit.

| Key | Value | Unit in the log | Fun sheet band key |
| --- | --- | --- | --- |
| `fm1_now` | the content drought: time since the last new content | s | FM-1 `max` |
| `fm1_h1` | the largest content gap in hour 1 so far | s | FM-1 `max_h1` |
| `fm1_gap` | the largest content gap so far | s | none |
| `fm2_last` | the income gain of the last hand purchase | % | none |
| `fm2_share` | the share of recent hand purchases below +5 % | share | FM-2 `max` |
| `fm3` | the distinct share of the purchase lines | share | FM-3 `min` |
| `fm4_now` | the time since the last income jolt | s | FM-4 `max` |
| `fm4_gap` | the largest gap between jolts so far | s | none |
| `fm6_chore` | the most hand presses on 1 line before its auto-buy | presses | FM-6 `max_presses` |
| `fm6_hand` | the hand share of the purchases in hour 1 | share | FM-6 `min_hand_share_h1` |
| `fm7` | the verbs of the current hour | verbs | FM-7 `min` |
| `fm8` | the last comeback ratio | ratio | none |
| `fm9` | the top-line share of the presses of the current hour | share | FM-9 `max` |
| `fm10` | the felt beat share | share | FM-10 `min` |
| `fm12` | the reset novelty share | share | FM-12 `min` |
| `fm16` | the tends each minute | per min | none |
| `pc2_now`, `pc2_gap` | the time since the last hand purchase, and its largest gap | s | none |
| `pc4_now`, `pc4_gap_h1`, `pc4_gap` | the time since the last unlock, and its largest gaps | s | none |
| `pc6` | the hand purchases in the last minute | purchases | none |
| `growth` | the income growth each minute, ln of the ratio | ln/min | none |

The script shows each value in seconds as minutes, because the fun sheet bands use minutes.

## Command line

```
python3 scripts/pw_playlog.py <folder> [--json FILE] [--md FILE] [--window S] [--bin M]
```

1. `<folder>`: the folder of the `.jsonl` files, for example the `playlog` folder of the user data of the game.
2. `--json FILE`: write the full result as JSON.
3. `--md FILE`: write the printed report as Markdown.
4. `--window S`: the window before each event, in real seconds (default 90). The value at an event is the last `fun` sample in the window. A mark carries its own sample.
5. `--bin M`: the bin width of the quit-point histogram, in minutes (default 10).

The script exits 0 with a result, and 2 with a broken line or a folder with no log.

## What the script computes

1. For each fun value: its median, minimum and maximum at the `good` marks, at the `drag` marks and at the quit points.
2. The quit-point histogram over real play time.
3. The settings of each session: the setting at the start, each `setting_load`, each `control_change`, and the setting at the quit point. The marks under each setting.
4. A proposed band edge for each value from the marks: the value that separates the `drag` marks from the `good` marks best. The script tries each midpoint between 2 adjacent values, in both directions, and keeps the one with the most marks on the correct side. It prints a sentence, for example "the F8 marks come after a content drought above 9.5 minutes (14 marks)".
5. A proposed band edge for each value from the quit points: the value that separates the quit points from the heartbeats of the same sessions before the window of the quit.
6. In `--json`, a block `targets.fun` that fits `game.targets.fun` of a model (the band keys of `scripts/pw_fun.py`). Each band has `band_mark` "play data (...)" with the counts.

The counts for a proposal come from section 6.5 of the research. They are inferred lower limits, not sourced.

1. Marks: 50 marks or more over 5 sessions or more, for 1 developer. Each side needs 5 marks or more with a value.
2. Quits: 20 quit points or more.
3. The edge must put 0.75 or more of the marks on the correct side (inferred).

Below these counts, the script prints "not enough data" with the counts, and gives no band. `targets.fun` gets a band only from the marks, only for a value with a band key, and only when the side of the `drag` marks agrees with the direction of the row: above the edge for a `max` key, below the edge for a `min` key.

## Limits

1. A proposed band is advice from a small sample. The report states the counts beside each proposal.
2. The marks of 1 developer show the taste of 1 person.
3. A quit has no reason. A quit at minute 40 can be a meal.
4. A value before more `drag` marks did not cause them. Only a comparison of 2 settings on similar players shows a cause.
5. A log covers 1 session. It does not measure why a player comes back on a later day.
