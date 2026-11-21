# Bundled skill scripts write no bytecode and import only their own code

The build bundles every file under `skills/`, so a script there must not
leave files in a skill folder, and it must import the modules beside it,
not ones from the working directory. `skills/skill-intake/scripts/intake.py`
sets `sys.dont_write_bytecode`, because the skill folder's digest covers
every file in it and a `__pycache__` would change it.

## Consequences

- `scripts/skill-intake/tests/base.py` sets `sys.dont_write_bytecode`, so
  importing `intakelib` from `skills/skill-intake/scripts` never writes
  `__pycache__` into the skill folder.
- `skills/video-audio/scripts/reconcile_timing.py` sets
  `sys.dont_write_bytecode`. Its self-test imports `check_timeline` from
  the video-storyboard skill, and a `.pyc` there would be bundled with that
  skill and break its size budget.
- `skills/incremental-game-design/scripts/progression_workbook.py` inserts
  its own directory into `sys.path`. The scripts run with `python3 -I`, and
  isolated mode does not add the script directory.
- The `skills/hark/scripts/hark` launcher exports `PYTHONSAFEPATH=1`, so
  `python -m hark` (Python 3.11+) does not put the current folder first on
  `sys.path`. The engine beside the script wins over any `hark` package in
  the working directory.
