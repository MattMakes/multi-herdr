# Engine live checks read failures where each engine reports them

The Blender and Unreal live checks do not trust the default exit code or
console output of the engine. They read the channel where each engine
actually reports a script failure.

## Consequences

- Blender returns 0 for a `--python` script that raises unless
  `--python-exit-code` is set. `scripts/live/blender-3d-print.sh`
  `run_blender` always passes `--python-exit-code 2`, and step 7 checks
  that a script that raises on purpose returns exit 2.
- Unreal's `unreal.log()` writes at Log verbosity. It reaches the
  `-abslog` file, not `-stdout`. `scripts/live/unreal.sh` checks the Python
  commandlet's markers and `LogPython` errors in the `-abslog` file.
