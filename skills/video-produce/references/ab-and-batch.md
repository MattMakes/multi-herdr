# A/B on 1 brief, and the batch factory

## A/B on 1 brief

2 directions share 1 brief, 1 storyboard, 1 capture set and 1 mix. Only the
look and the motion differ. A fresh critic ranks the 2 masters without
knowing which direction made which.

1. Waves 1 to 3 run once in the project folder `video/<slug>/`. In wave 1
   pick 2 of the 3 directions by their ids in `DIRECTION.md` (`D1`, `D2`,
   `D3`). Write both in `PRODUCTION.md`. The `chosen:` line of the project
   folder names the 1st pick, because the plan gate needs exactly 1.
2. Fork after wave 3: copy the project folder to `video/<slug>-<id>/` once
   for each direction id, for example `video/launch-v3-d2/`. Each fork keeps
   all 3 directions in `DIRECTION.md`, because the plan gate checks them.
   Ask the orchestrator to set the fork's `chosen:` line to its own
   direction. Do not copy `.video/` into a fork.
3. Run waves 4 and 5 in each fork. Each fork must pass its own wave 4 and
   wave 5 gates.
4. Assign the labels, from the project folder. Pick any integer seed:

   ```bash
   python3 "<skill-dir>/scripts/ab_blind.py" assign --seed <n> \
     --map .video/ab-labels.json --out-dir render/ab \
     D1=../<slug>-d1/render/master.mp4 D3=../<slug>-d3/render/master.mp4
   ```

   The seed gives the same labels every time. `.video/ab-labels.json`
   records the seed and which direction is A and which is B.
5. Write the ranker's plan. Inputs: `BRIEF.md`, `STORYBOARD.md`,
   `timeline.json`, `render/ab/A.mp4`, `render/ab/B.mp4`. Do not give it
   `DIRECTION.md`, a fork folder or `.video/`: they name the directions.
   Lint the plan before you send it:

   ```bash
   python3 "<skill-dir>/scripts/ab_blind.py" lint-plan --map .video/ab-labels.json <plan.md>
   ```

   Exit 1 means the plan names a direction, a source path or `.video/`.
   Remove the name and lint again.
6. A fresh `video-critic` ranks the 2 masters with the ranking mode of the
   `video-qa` rubric and writes `qa/ab-rank.md`.
7. Check the rank and read the result:

   ```bash
   python3 "<skill-dir>/scripts/ab_blind.py" check-rank qa/ab-rank.md --map .video/ab-labels.json
   ```

   It prints `preferred <label> = <direction id>`. Write the preferred
   direction and the margin per axis in `PRODUCTION.md`.
8. The preferred fork goes on to the wave 6 critic loop. Keep the other
   fork until the film ships, then tell the orchestrator it can go.

`lint-plan` matches each direction id and each source path as a whole
word, in any case. Use the `D<n>` ids. A direction's name, such as
"Calm drift", never goes into a ranker's plan either: check it by eye.

### A new version of 1 film

Use the same commands with the ids `current` and `candidate`. The
candidate replaces the current master only when the rank prefers it.

## Batch factory

A batch makes N films with 1 shared brand, 1 voice and 1 notes file. Each
film passes every gate on its own.

### Manifest

```json
{
  "batch": "weekly-2026-45",
  "shared": {"brand": "brand/BRAND.md", "voice": "Samantha",
             "notes": "/Users/me/.local/state/horch/video/NOTES.md"},
  "films": [
    {"slug": "launch-v3", "request": "30 s launch film for the CLI",
     "workflow": "launch-film", "folder": "launch-v3",
     "length_sec": 30, "aspects": ["16:9", "9:16"]},
    {"slug": "cli-demo", "request": "show the ledger in the terminal",
     "workflow": "app-explainer", "folder": "cli-demo",
     "length_sec": 60, "aspects": ["16:9"]}
  ]
}
```

Keep the manifest in the batch folder, for example
`video/batches/weekly-2026-45/batch.json`. Each `folder` is relative to the
manifest folder. `workflow` is `launch-film`, `app-explainer` or `ab`. A
batch never holds a batch.

### Run

1. `validate_batch.py <batch>.json` exits 0.
2. For each film, write its `PRODUCTION.md` from the manifest row and the
   shared values. Load the notes from `shared.notes`. Give
   `shared.voice` to `video-audio` and `shared.brand` to `video-director`.
3. The orchestrator runs the recipe for each film. Films may run side by
   side when the fleet has free seats. Each film has its own critic loop.
4. After each film: harvest, the share kit, then
   `validate_batch.py <batch>.json --summary <batch>-summary.md`.
5. Report the summary table to the orchestrator. A film with status `open`
   names its last verdict and its critique file.

The summary row of a film:

```markdown
| Film | Workflow | Rounds | Verdict | Average | Master | Share kit | Status |
| launch-v3 | launch-film | 2 | PASS | 4.25 | yes | yes | shipped |
```

### Weekly start

The operator's own scheduler (`cron`, `launchd`, or a scheduled agent run)
starts the orchestrator with the manifest path. The producer installs no
schedule and writes no system file.
