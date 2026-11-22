# Fixture: the built game Moss Works

This folder is a copy of some files of the built Godot game Moss Works. The scan
`scripts/scan_godot.py`, its tests and worked example 1 of `scripts/verify_math.py` read it.

Source: the repository `moss-works`, commit `e3221e75e6a3940c482de32b4535933ff9709201`
("feat(economy): bulk purchase for upgrades, beds and tonics with BUY_MAX"). That commit is the
last change of these files at the time of the copy (2026-11-14).

Files:

- `project.godot`
- `systems/economy.gd`, `systems/garden.gd`, `systems/save.gd`, `systems/settings.gd`
- `data/garden.json`, `data/settings_defaults.json`, `data/stages.json`, `data/upgrades.json`

The scenes, the UI scripts and `systems/sfx.gd` are not in the copy. The autoload `Sfx` in
`project.godot` names a file that is absent. The scan does not need it.

The empty file `.gdignore` stops the Godot import of this folder. Do not edit the files: the
tests compare the scan output with known values of the built game.
