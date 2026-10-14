---
name: godot-game-audio
description: >-
  Make, wire and measure a Godot game's audio by numbers: seeded procedural sound effects and
  ambient loops that render the same bytes each time, a file report (length, LUFS, peak, onset,
  leading silence, loop seam), a bounded audio autoload with tests, a played-mix loudness
  measure from Movie Maker recordings (integrated LUFS, true peak, momentary max, duck depth),
  and music sets made by script or by an AI music web service that must loop, hit loudness and
  tempo targets and render deterministically. Use it whenever someone wants to add or change
  sound effects, generate SFX in code, fix a click at a loop seam, tune buses, ducks or a
  limiter, check that the mix is too loud or too quiet, re-check audio review findings, or add
  a soundtrack or a second music set, even if nobody says "loudness" or "procedural audio".
---

# Game Audio

Target engine: **Godot 4.7**. The Python block in `references/script-wav_report.md` uses the
standard library only.

Game audio drifts when nobody measures it. A regenerated sound changes without notice, a loop
clicks at its seam, a reward cue is quieter than a footstep, and a mix change "sounds better"
on one pair of speakers only. This skill treats every sound as a build product with numbers.
A seeded script makes the files, and a report proves each file meets its rules. A recording of
the real played mix proves the buses meet their loudness targets. It works because the numbers
catch regressions that ears miss, and a human listen pass is still left for the things that
numbers cannot judge.

## When to use

- You add, change or remove a sound effect or an ambient loop that a script generates.
- You wire a new game event to a sound, or change the audio autoload.
- You tune buses, ducks (sidechain compression), a limiter, or bus volumes.
- An audio review found problems, and the fixes landed. You must re-check them.
- You add a music set, edit album tracks to fit the game, or generate music with a web service.
- A loop clicks, a cue starts late, or a file plays at the wrong length.

## When not to use

- The game uses only recorded or licensed sound files and you do not change them. Use the mix
  section only.
- You need audio design taste (what a cue should feel like). Numbers check a design; they do
  not make one.
- You need middleware such as FMOD or Wwise. The rules still apply, but the wiring differs.

## Method

This is the procedure for scripted sound effects and ambient loops. Each step names the check
that proves it.

1. **Write the sound list.** List each event, its cue name and the seam (the code site) that
   plays it. Agree the event contract with whoever owns the event bus: a one-shot carries a
   sound id and a position; a loop has an on event and an off event. Check: the sound names in
   the list match the names that the code uses. The names are the contract between builders,
   so do not rename them casually.
2. **Snapshot the current files.** Hash every existing sound file (md5 or sha256), or copy the
   folder to a "before" folder. Run the file report on them and keep the table. Check: you
   have a hash list and a before table.
3. **Prove the generator is deterministic before you edit it.** Run the generator once with no
   edits. Check: version control shows no change to any committed sound file. If files change,
   the generator is not seeded; fix that first, or every later diff lies.
4. **Write a failing file test.** The test expects the new files to exist and to pass the file
   rules. Check: the test fails for the right reason (the file is missing).
5. **Add the recipes.** Give every noise source a fixed seed (for example seed by the cue
   name). Add new recipes in a new file or at the end of the old one, so old seeds do not
   shift. Normalize each cue to its loudness target and clamp the peak under a ceiling.
   Render loops as circular buffers, so the end flows into the start.
6. **Render twice and compare.** Render into 2 empty folders and compare the bytes. Check: the
   2 hash lists are equal, and every old file keeps its old hash. Only new or intentionally
   changed cues may differ.
7. **Run the file report at the real sample rate.** Read the rate from the file header; never
   assume 44.1 kHz. Check each rule:
   - length within its budget;
   - loudness (LUFS) near its target, and the peak under the ceiling;
   - leading silence under 1 ms for a one-shot, so the sound lands on the event frame;
   - a sharp onset for impact cues;
   - a near-silent last sample for a one-shot, so it does not click when it stops;
   - for a loop, the seam rules in `references/loop-seam.md`.
   Fix each recipe that breaks a rule and run the report again.
8. **Wire the cues in one audio autoload.** Only that autoload creates players; check with a
   grep for player nodes elsewhere. Use a bounded pool of players, so a burst of events cannot
   grow the node count. Create 0 players when the game runs headless, so tests and bots stay
   quiet and cheap. Write runtime tests: each event reaches the right player, the pool stays at
   its bound, an unknown id plays nothing, and headless mode has 0 players.
9. **Import twice.** Run the editor import headless, then run it again. Judge only the second
   pass, because a new file can fail to load on the first pass. Check: each loop's `.import`
   file keeps its loop setting.
10. **Set loop ends from the stream.** Read the loop end from the stream length at run time,
    not from a constant. A compressed import (for example QOA) can change the frame count.
11. **Find loops by a name rule.** A loop is a file whose name matches one rule (for example a
    `loop_` prefix). Tests and tools use that rule, not a hand-kept table, so a new loop does
    not break 2 tests.
12. **Run the whole audio test folder, then the full suite.** A focused file test can pass while
    a rule in another audio test fails (for example onset or last-sample rules).
13. **Leave the listen pass open.** Write a listen checklist: each new cue, what to listen for,
    and where it plays. Report the human listen pass as open, not as done.

## Measure the played mix

File numbers do not prove the mix. Record the real game with Movie Maker mode and meter the
play part against a per-bus audio contract. The 10 steps are in `references/played-mix.md`.

## Measure a loop seam

This skill owns loop-seam measurement. Other skills (for example `godot-release-and-store` for
a trailer music bed) call it instead of a second procedure. The 5 steps and the 2 ratio rules
are in `references/loop-seam.md`.

## Music sets

A music set is a group of tracks for areas, fights and menus, made by script, from edited album
tracks or by an AI music web service. The 11 steps and the web service rules are in
`references/music-sets.md`.

## Pitfalls

The full table of 27 rows is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| A 48 kHz file reports 9 % too long | The report tool assumed 44.1 kHz | Read the rate from the header, or pass it |
| A one-shot feels late | Leading silence (for example 36 ms) in the recipe | Trim the start under 1 ms |
| A loop clicks once per cycle | Seam step above the 99th-percentile step | Render as a circular buffer; match the end values |
| WAV seams pass but the game clicks | Codec error at the edges of the encoded file | Measure the decoded file; render again |
| An 8 s loop stops at about 3 s | A fixed loop end after a compressed import | Set the loop end from the stream length at run time |
| A new file fails to load | First import pass | Import twice; judge the second pass |
| The mix recording is all zeros | A real-time record effect on a multi-channel device | Use Movie Maker mode |
| Reward tiers do not rise in loudness | Gated loudness on short cues; peak normalization | Use momentary max; normalize by loudness |

## Artifacts to build

- **Sound generator script.** Seeded synthesis helpers (oscillators, noise, envelopes,
  filters), one recipe per cue, a loudness normalizer with a peak ceiling, and an output
  folder argument so you can render twice.
- **File report.** One JSON line per file with the fields in step 7.
  `references/script-wav_report.md` holds a standard-library version. Copy the block into a
  file `wav_report.py` and run `python3 -I wav_report.py --selftest` first.
- **Hash compare.** A short script or shell line that hashes 2 render folders and prints any
  file that differs.
- **Audio autoload.** The only owner of players: a bounded pool, one-shot and loop entry
  points, a silent path for an unknown id, and no players in headless mode.
- **Audio tests.** File tests (existence, rules, the loop name rule) and runtime tests (event
  routing, pool bound, unknown id, headless).
- **Mix capture scene or script.** It plays a fixed scene under Movie Maker mode and writes an
  event log with timestamps, so the meter can cut the play part.
- **Mix meter.** Integrated LUFS, true peak, momentary max, per-event levels, gaps and duck depth,
  printed as a PASS or FAIL table against the audio contract.
- **Audio contract.** File names, buses, targets per bus, size budget and the loop rule.
- **Music config or brief.** Cue to source, tempo scale, loop start, area tempo and key family.
- **Takes index.** For a web service: cue, prompt id, take links and the chosen take.
- **Listen checklist and recheck file.** One row per cue or review finding, with a result.

## Related skills

- `godot-headless-runs`: the safe flags and cleanup for headless imports and tool runs.
- `godot-test-loop`: the test loop, how to prove a test can fail, and the finish gate.
- `godot-visual-verification`: windowed captures; a Movie Maker recording needs a real window too.
- `godot-feature-seams-and-saves`: ship a new music set behind a settings choice and a data seam.
- `godot-release-and-store`: the trailer and store audio use the same loudness numbers, and its
  music-bed seam check calls "Measure a loop seam" in this skill.
- `godot-fleet-game-dev`: shared-file waits, shell pitfalls and when to ask before a paid step.
- `godot-sound-design`: what a feedback cue is, and its loudness, length and latency rules.
- `godot-audio-system`: the engine side: players, the bus API, pools and music.

## References

- `references/played-mix.md`: read when you record and meter the played mix or tune buses.
- `references/loop-seam.md`: read when you check a loop seam, on a WAV or on an encoded file.
- `references/music-sets.md`: read when you add a music set or use an AI music web service.
- `references/pitfalls.md`: read when a symptom is not in the short Pitfalls list.
- `references/script-wav_report.md`: read when you need the file report script.
