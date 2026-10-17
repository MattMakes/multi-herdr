# Pitfalls: full table

`SKILL.md` keeps the 8 most important rows. This file holds every row.

## Pitfalls

| Symptom | Cause | Fix |
| --- | --- | --- |
| A text or CSV file is missing in the exported game | The preset include filter lists `*.json` only | Add each needed file type to every preset filter |
| The demo pack lists full-game or trailer paths | The UID cache keeps paths the plugin skipped | Audit the UID cache; complete the skip list |
| The audit finds a `.gdc` under a full-game folder | Compiled scripts escape a `.gd` glob | Skip the whole folder |
| The demo fails to start with a parse error | A kept script `preload`s a cut file | Load it only when `ResourceLoader.exists` |
| A shared item is gone in the demo | The row filter cut a shared key | Exempt shared names |
| The demo and the full game share saves | The user folder follows the project name | Decide on purpose; set a custom user folder; test carry-over |
| `-s tool.gd` does nothing on the exported app | Exported apps ignore `-s` | Run tools from the editor binary |
| The Linux preset fails | The platform name changed with the engine version | Use the name the binary reports |
| A template download stops near half | 1 long HTTP stream stalled | Ranged or parallel requests, then the checksum |
| The universal macOS export fails | ETC2/ASTC import is off in the project | Ask the project file owner to turn it on |
| Bad saves throw parse errors in release | No save hardening | See `godot-feature-seams-and-saves` |
| Only the first pad works | New joypad events default to device 0 | Device -1 for shared actions |
| A test press never reaches `_unhandled_input` | `Input.action_press` skips the event path | `Input.parse_input_event` |
| A button acts on release in tests | Only the press event was sent | Send press and release |
| The second tab on 1 edge is unreachable by pad | Focus picks the first tab bar | Choose the tab of the focused group, else the nearest |
| A label covers its glyph | Icon placed after the text | Icon on the left when text is present |
| A checklist item fails in a file you do not own | Project default outside scope | Ask 1 question; do not edit |
| A re-assembly needs a new recording | Raw frames were deleted | Keep raw frames until the cut is final |
| A single-trailer run deleted screenshots | Cleanup took a shared folder | Clean only the run's own temp folder; regenerate |
| A clip ends early or takes under 1 s | The scene ended or crashed | Record again at a longer length; check the time |
| The assembler fails on a frame pattern | A clip was not recorded | Record the clip, then assemble again |
| Tutorial hints show in the trailer | Hints hide only on real input events | Send input events in the shot script |
| A mockup tag shows on store art | It was removed by hand once | Remove it in the art script |
| The watcher fires on every ffmpeg line | It matched a common word | Match step header lines only |
| Blank frames in a recording | Movie Maker ran headless | Record in a real window |
| A store claim names absent content | Text not checked against data | Check each claim against the game data |
| 2 docs give different waits | The docs disagree | Plan with the safe value; log the conflict |
| A rule has no source | Docs are silent | Mark it "not confirmed" |
| `sed -i` or `sed 1i` fails on macOS | BSD `sed` syntax | Use a heredoc or a Python edit |
| A script fails on a missing image or array library | Wrong Python, or the library is absent | Use the right interpreter, or the standard `array` and `wave` modules |
