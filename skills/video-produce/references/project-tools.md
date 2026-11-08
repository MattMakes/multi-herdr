# Project tools and skill scripts

A project script can call a script supplied by a skill, but it must resolve the
script from the installed skill root every time it runs. Use
`"$(horch skills path <id> --script <rel>)"`. Never save or copy a path that
contains `skill-bundles`; horch can replace and remove those temporary copies
between production runs.

Make the project script exit non-zero when `horch skills path` cannot resolve
the skill or script, and when the resolved script exits non-zero. For example,
a shell build script can use:

```sh
set -e
python3 "$(horch skills path video-audio --script scripts/check_audio.py)" \
  audio/mix.wav --timeline timeline.json --out qa/audio-check.json
```

Do not catch either failure and continue. A required gate that did not run is
a failure, not a pass. Record a pass only after the resolved gate script runs
and returns exit status 0.
