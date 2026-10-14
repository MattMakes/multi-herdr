# The output files: `balance.json` (schema `skill-tree-balance/1`) and `wall.json` (schema `wall-probe/1`)

`balance.json` is the end product of the method. The report tool writes it from the traces, the wall probes and the
chapter targets. A designer (or an agent) reads it to build and price the real skill tree of each class or vehicle.
Every number has a definition below; keep them exact, so that 2 runs of the method compare. The ids and numbers in the
2 examples are sample values only (`chapter_1`, `boss_a`, `class_a`, round minutes); they come from no real game.

## balance.json

```json
{
  "schema": "skill-tree-balance/1",
  "generated_utc": "2026-10-12T23:00:00Z",
  "inputs": {"traces": ["sweep/balanced/trace.json"], "walls": ["walls/boss_a/wall.json"], "targets": "targets.json"},
  "near": 0.10,
  "baseline": "balanced",
  "policies": [
    {"name": "balanced", "skill": "strong", "traces": 3, "progress": 6,
     "total_min_mean": 100.0, "total_min_min": 90.0, "total_min_max": 110.0, "rank": 1, "near_optimal": true}
  ],
  "chapters": [
    {"id": "chapter_1", "order": 1, "target_min": 20.0, "minutes": {"balanced": 21.0, "fight": 18.0},
     "best_policy": "fight", "mark": "OK"}
  ],
  "spokes": [
    {"id": "speed", "kind": "stat", "group": "", "loo_worth_min": 10.0, "loo_progress_lost": 0,
     "level5_min": {"balanced": 8.0}, "closed_in": ["balanced"],
     "wall_breaks": [{"boss": "boss_a", "bump": 3, "health_delta": -0.10, "killed": false}]}
  ],
  "groups": {
    "class_a": {"chapters": ["chapter_1"], "spoke_order": ["tick_pct", "speed"], "level_gain": {"tick_pct": 6, "speed": 4}}
  },
  "currencies": {
    "chapter_1": {"gems": {"haul_median": 0.5, "bank_median": 1.0, "bank_max": 3.0, "paid_share": 0.05}}
  },
  "walls": [
    {"boss": "boss_a", "try": 3, "baseline_health_left": 0.80,
     "top": [{"spoke": "dmg_pct", "bump": 3, "health_delta": -0.40, "killed": false}]}
  ],
  "notes": []
}
```

### Definitions

- **progress**: the count of distinct major bosses killed in a trace. A policy's `progress` is the minimum over its
  traces (a policy is only as good as its worst seed).
- **total minutes** of a trace: `game_s / 60` of the run with the last first kill of a major boss; with no major kill,
  `game_s / 60` of the last run. `total_min_mean`, `_min`, `_max`: over the policy's traces.
- **rank**: policies sorted by `progress` (high first), then by `total_min_mean` (low first). **near_optimal**: the
  policy has the top progress and `total_min_mean <= (1 + near) x` the best `total_min_mean`.
- **chapter**: the values of the run field `chapter` (levels, zones, acts) in the order they first appear in a trace.
  If your traces use another field name, pass it with `report.py --chapter-key <name>`. The minutes of chapter k in a
  trace: the `game_s` of the last run before chapter k+1 first appears, minus the `game_s` of the run before
  the first run of chapter k (0 for the first chapter). `minutes[policy]` is the mean over the policy's traces that
  reached chapter k+1 (the last chapter: traces that killed its major boss).
- **target_min**: from the targets file (`{"chapter_1": 20.0, ...}`); absent = null. **mark**: for the baseline policy,
  "OK" within +-15 % of the target, "SLOW" above, "FAST" below, "" with no target. A calibration that cannot meet
  the target writes "WALL" into the targets file entry (`{"chapter_1": {"min": 20.0, "mark": "WALL"}}`); the report copies it.
- **loo_worth_min**: for the leave-one-out policy that skips this spoke (policy `skip` = [spoke], `baseline` = the
  baseline): its `total_min_mean` minus the baseline's. Positive = the game is slower without the spoke = the spoke is
  worth that many minutes. When progress differs, `loo_worth_min` is null and `loo_progress_lost` = baseline progress
  minus the leave-one-out progress.
- **level5_min[policy]**: the mean game minutes at which the spoke first reached level 5 (null if never).
- **closed_in**: the policies in which a `spoke_closed` event fired for this spoke (a game cap stopped it).
- **wall_breaks**: from the wall probes (below), every probe of this spoke, sorted by `health_delta` (most negative
  first).
- **groups**: 1 entry per value of the trace run field `group` (a class, character or vehicle). If your traces use
  another field name, pass it with `report.py --group-key <name>`; the spoke field of the same name gives `spokes[].group`. `chapters`: the chapters with
  runs of that group. `level_gain[spoke]`: the levels bought during runs of that group, summed over the near-optimal
  policies' traces, divided by their trace count. `spoke_order`: the spokes by `level_gain` (high first); a tie goes to
  the spoke bought first.
- **currencies[chapter][currency]**: over the runs of that chapter in all traces: the median `haul`, the median and
  the maximum of `bank_before`, and the share of `paid` in all paid amounts of the chapter.
- **walls**: 1 entry per wall probe file; `top`: the 5 probes with the most negative `health_delta`.
- **notes**: free text lines that the agent adds after it reads the numbers (for example "2 near-optimal policies,
  both open with tick_pct": the hint for the real tree's first ring).

## wall.json (written by the wall finder, read by the report)

```json
{
  "schema": "wall-probe/1",
  "boss": "boss_a",
  "try": 3,
  "snapshot": "snapshots/try-boss_a-3-run9.json",
  "baseline": {"health_left": 0.80, "result": "run_ended", "fight_s": 30.0},
  "probes": [
    {"spoke": "dmg_pct", "bump": 3, "health_left": 0.40, "health_delta": -0.40, "result": "run_ended", "fight_s": 30.0}
  ]
}
```

- A probe replays the same boss try from the same save with `levels[spoke] += bump` (a probe, not a buy: the price
  count N does not change). `health_delta` = probe `health_left` minus baseline `health_left`.
- The baseline replay must equal the original try. If it does not, the game is not deterministic for this setup; fix
  that before you trust any probe.
