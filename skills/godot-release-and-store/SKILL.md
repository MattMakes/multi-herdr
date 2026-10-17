---
name: godot-release-and-store
description: >-
  Ship a Godot game: export release builds for desktop and Web from the command line, cut a
  demo or playtest edition from the same project and prove no full-game or dev file leaks into
  it, make the game fully playable on a gamepad or handheld PC with the right button glyphs,
  record repeatable gameplay trailers and store screenshots from scripts, and research the
  current store rules for a cited launch guide and store page draft. Use it whenever someone
  wants to export, package, zip or smoke-test a build, make a demo, add controller or Steam
  Deck support, remap buttons, show pad glyphs, cut a trailer with Movie Maker, make capsule
  art or screenshots, or asks "what do I need to launch on Steam?", even if nobody says
  "release". Use it again after gameplay changes, because trailers, screenshots, store text
  and the demo cut all go stale.
---

# Release and store

Target engine: **Godot 4.7**. The script references hold Python blocks that use the standard library only.

A game that plays well in the editor is not yet a product. The exported build can miss data
files, a demo can ship the full game inside its pack, a pad can fail to reach a menu, and the
store page can promise content the game does not have. This skill treats each release output
(builds, demo, input, trailer, store page) as a scripted artifact with a check that proves it.
Scripts make every output again in 1 command after the game changes, and the checks catch the
leaks and gaps that a person does not see by eye.

## When to use

- You export a Godot game for 2 or more platforms, or for the Web.
- You ship a demo, a playtest build or another edition from the same project.
- The game must play fully on a gamepad or a handheld PC.
- You need a gameplay trailer, share copies, store screenshots or store art.
- You need a current, cited guide to a store launch, or a store page draft.
- Gameplay or rules changed, and the release media must be made again.

## When not to use

- You harden save files against bad data. Use `godot-feature-seams-and-saves`.
- You inject input in unit tests only. Use `godot-test-loop`.
- You need a window capture review of a UI screen. Use `godot-visual-verification`.
- You make the music or sound for the trailer. Use `godot-game-audio`.

## Method: export builds and editions

1. Read the engine version with `godot --version`.
   The export templates must match it exactly, including the status suffix (for example
   `4.3.stable`).
2. Download the export templates archive and the official checksum file for that version.
   If the download stalls, resume it with HTTP range requests (`curl -C -`), or fetch parts in
   parallel and join them.
   Check the SHA-512 sum before you unzip. It proves that a resumed download is whole.
3. Unzip the templates into the user template folder for that version.
   Check: an export with no template error.
4. Write the export presets in the editor or in `export_presets.cfg`.
   - Set the Web preset to the Compatibility renderer with thread support off. Most web hosts
     do not send the cross-origin isolation headers that threads need.
   - For a universal macOS build, turn on the ETC2/ASTC texture import setting in the project.
     This is a project setting, so ask its owner if you do not own the project file.
   - Use the platform name that your engine binary reports. Names change between versions.
   Check: `godot --headless --export-release "<preset>" <out>` passes for each preset.
5. Add every non-JSON data file type to the include filter of every preset.
   The default export takes resources and scripts only. A plain text, CSV or credits file stays
   out with no error.
   Check: search the exported pack for 1 path of each data type (step 11).
6. Write 1 export script that runs each preset into a staged folder.
   - Name each file by version and commit.
   - Print each file size.
   - Warn when the tree has uncommitted changes, because the commit in the name then lies.
   Check: the script exits 0 and prints 1 line per preset.
7. Stamp the build with the version and the commit in a small generated file.
   Show the stamp on the title screen. A tester can then report the exact build.
8. Mark each edition (demo, playtest, full) with a custom feature in its preset.
   Read it at run time with `OS.has_feature("demo")` inside 1 gate script.
   Every screen asks the gate. Do not test the feature in many places.
   Custom features exist only in exported builds. Give the gate an override (a command-line
   argument or a setting) so the editor and headless tests can act as each edition.
   Check: a headless test of the gate for each edition.
9. For a demo cut, follow `references/demo-cut.md`.
10. Gate store features behind a platform backend seam (an interface with a real backend and a
    fake backend for tests).
    Decide achievements and rich presence for each edition and each store as 2 separate
    decisions. Base each decision on the current store guidance that you cite
    (`references/store-launch-research.md`). Store rules differ and change.
    For example, a store guide can say that a demo unlocks no achievements but keeps rich
    presence. Keep that only when your cited guidance says it.
    Put each decision in data that the backend seam reads, not in many screens.
    Check: a test for each edition that the backend seam allows exactly the unlocks and the
    presence that the decision allows.
11. Audit each built pack.
    - Search it for forbidden paths, content ids and dev files (tests, tools, trailer scenes).
    - Search the UID cache file too. It lists paths of files that are not in the pack.
    - Search it for 1 required data path, to prove the data is inside.
    Check: the audit exits 0. See `references/script-pack-audit.md`; copy the block into a file.
12. Unzip each archive in a new temp folder and start the app once with an empty `HOME`.
    Use `--headless --quit-after 60` and expect exit 0.
    An empty `HOME` proves the build does not depend on your own save or settings.
    An exported app ignores `-s`, so you cannot start a tool script inside it.
13. Decide on purpose whether editions share the user folder.
    Godot names the user folder from the project name, so a demo and the full game share saves
    by default. For an edition that must not share, set a custom user folder name, for example
    with a feature-tag override of the custom user folder project setting.
    Check: copy a save in, start the other edition, and compare the save files before and after.
    A load can rewrite a save after a migration.
14. Wrap play, test, export and dist in task-runner recipes, with an optional edition
    argument.
    Move old archives to an old folder, so an upload never takes a stale zip.

## Demo cut by export plugin

The demo cut with an `EditorExportPlugin`, guarded loads and the pack audit is in
`references/demo-cut.md`.

## Gamepad and handheld support

The input hub, pad events, glyphs, dead zone, focus, test injection and the handheld check tool
are in `references/gamepad-and-handheld.md`.

## Trailers and store media

The Movie Maker clips, contact sheets, assembly, encodes, stills, screenshots and store art are in
`references/trailers-and-store-media.md`.

## Store launch research

The cited research method for store rules and the store page draft is in
`references/store-launch-research.md`.

## Pitfalls

The 8 most important rows. The full table is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| A text or CSV file is missing in the exported game | The preset include filter lists `*.json` only | Add each needed file type to every preset filter |
| The demo pack lists full-game or trailer paths | The UID cache keeps paths the plugin skipped | Audit the UID cache; complete the skip list |
| The demo fails to start with a parse error | A kept script `preload`s a cut file | Load it only when `ResourceLoader.exists` |
| The demo and the full game share saves | The user folder follows the project name | Decide on purpose; set a custom user folder; test carry-over |
| `-s tool.gd` does nothing on the exported app | Exported apps ignore `-s` | Run tools from the editor binary |
| Only the first pad works | New joypad events default to device 0 | Device -1 for shared actions |
| Blank frames in a recording | Movie Maker ran headless | Record in a real window |
| A store claim names absent content | Text not checked against data | Check each claim against the game data |

## Artifacts to build

- Export script: runs each preset into a staged folder, names files by version and commit,
  prints sizes, warns on a dirty tree.
- Build stamp: a generated file with version and commit that the title screen reads.
- Edition gate script: 1 function per rule, read from the custom export feature, with a
  headless test.
- Demo export plugin and demo set: an `EditorExportPlugin` plus a data file of kept rows and
  skip globs.
- Pack audit: forbidden and required strings checked against each pack and the UID cache
  (`references/script-pack-audit.md`; copy the block into a file).
- Smoke runner: unzips each archive in a temp folder and starts it with an empty `HOME`.
- Platform backend seam: an interface, a real backend, a fake backend, and an achievements
  data file generated from game data.
- Input hub: an autoload with defaults, event encoding, device family and glyph lookup; a
  prompt control; 1 glyph sheet per family.
- Handheld check tool: runs at the handheld size with a pad override and saves captures per
  screen and family.
- Trailer chain: shot scripts with seeds, a record-all script, an assembly script with an
  output-folder flag, a screenshot picker and a store art script.
- Store guide: a table of facts, each with a source tag and a "not confirmed" mark where
  needed; a research log; a store page draft.
- Task-runner recipes: play, test, export, dist, demo, trailers.

## References

- `references/demo-cut.md`: read it when you cut a demo or another edition from the project.
- `references/gamepad-and-handheld.md`: read it when you add pad, remap, glyph or handheld support.
- `references/trailers-and-store-media.md`: read it when you record trailers or make store media.
- `references/store-launch-research.md`: read it when you research store rules or draft the store page.
- `references/pitfalls.md`: read it when a release step fails and the short table has no row for it.
- `references/script-pack-audit.md`: copy the block into a file to audit a pack for forbidden and required strings.
- `references/script-html-to-text.md`: copy the block into a file to convert saved HTML pages to text.
- `references/script-loop-seam.md`: copy the block into a file to measure the loop seam of a 16-bit WAV file.

## Related skills

- `godot-feature-seams-and-saves`: save hardening, save migration and flagged modes; this skill
  only decides whether editions share saves.
- `godot-headless-runs`: headless flags, tool scripts and the task runner that the export
  recipes use.
- `godot-test-loop`: the test loop and input injection in tests; this skill covers the pad
  design.
- `godot-visual-verification`: the window lock, capture queue and capture review for handheld
  checks and trailer recordings.
- `godot-game-audio`: the music bed and mix loudness for the trailer.
- `godot-pixel-art-pipeline`: palette-locked art for title cards and store art.
- `godot-fleet-game-dev`: sandboxed workers cannot record windows; give recordings to a worker
  that can.
