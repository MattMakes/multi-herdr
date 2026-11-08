# Game SFX with hark

Game sounds differ from film sounds in 3 ways: the game triggers them, often many times a minute; the engine mixes
them at run time; and a loop must join with itself. hark makes, picks and checks the assets. The `godot-sound-design`
skill owns the engine side: latency, buses, the event-to-sound map, voices, ducking and the in-game audit. Follow its
rules; this file says how hark serves them.

## 1. List the events

Write 1 row per game event: name, rate (events per second at its busiest), whether it carries a value (a reward tier,
a combo step), and the sound need in source-material-action words ("a boot on wet gravel", not "player step"). The
rate sets the length: more than 8 per second, 30-90 ms; 2-8 per second, at most 150 ms; rare rewards 400-1000 ms
(godot-sound-design rule 6).

## 2. Get the sound

| source | when | commands |
|---|---|---|
| bank | the need fits a bank family | `hark sfx list --kind <kind>`, `hark sfx render <ID> -o x.wav --seed N [--pitch NOTE] [--dur S] [--sr 44100]` |
| kit | a set of roles with takes and a lock file | `hark kit init`, edit roles and `takes`, `hark kit render kit.json --out sfx/kit` |
| forge | a sound nothing in the bank covers | `forge.md`, loop 4 |
| fit | the user has a CC0 recording to turn into code | `hark fit <file> -o recipe.py --takes` |
| recordings | the task calls for real recordings | the `sound-sourcing` skill (sourcing, rights audit, credits) |

Pick between candidates with `hark rank` and `hark ab` (`listening.md`), and judge each by measurement, never by the
critic score alone.

## 3. Variation pools

A repeated action draws from a pool so it never machine-guns:

- at least 4 takes per pool (footsteps, hits, UI taps, impacts); 8-12 for very frequent sounds such as steps;
- the engine never plays the same take twice in a row (cycle or a shuffle bag);
- level jitter +-1-2 dB;
- pitch jitter at most +-0.3-0.5 semitones for real materials (a real key or boot keeps its pitch: vary the take);
  up to +-0.5-1 semitone for abstract UI sounds that play more than once per second;
- a sound that carries a value (a tier, a combo step) does not vary; a combo steps up a scale and resets on a break.

Takes come from `sfx render --seed N` (1 seed per take) or from a kit role's `takes`. `kit.lock.json` holds onset,
peak, end and loudness per take: use it to trim takes and to keep pool levels even (gain per take = pool target minus
the take's loudness).

## 4. Check every asset

- `hark sfx check sfx/*.wav [--category C] [--role R]` audits any effect file. `hark kit check kit.json --out DIR`
  audits kit takes. Their warnings (clicks, truncated tails, silence, pitch drift, 2 attacks) are the defects that
  ruin a rapidly retriggered sound. Replace or re-render a take with a warning.
- Sharp onset: leading silence at most 1 ms, and the sound reaches 20 dB under its peak within 5 ms. Read `onset` in
  the lock, or `attack_ms` from `hark measure`; trim the head when it is late.
- Edges: no click at the start or end. A fade-in of 1-3 ms and a fade-out that never cuts the decay.
- Measure each asset with `hark measure` and keep the readings in the asset manifest.

## 5. Loops (ambience beds, engine hums)

- Cut the loop from a clean window: `hark find <bed> "<texture description>" --kind texture` gives windows meant to be
  crossfaded; `hark measure` on the window shows its level spread.
- Join the end to the start with an equal-power crossfade (square roots of raised-cosine curves) of 1-1.5 s for beds,
  shorter for hums.
- Judge the seam by measurement: render 2 loop cycles back to back and check the join. The level steps by less than
  the window's own spread, no 14 kHz click appears at the seam, and no single loud event repeats every cycle (an
  audible loop).

## 6. Loudness for games

Films master to -14 LUFS integrated. Game assets do not: the engine mixes them at run time.

- Normalise per asset class, not per file to 1 integrated target. Keep 1 sample-peak ceiling for every one-shot
  (`sfx render --peak -1.0` is the default) and set the class level in the engine's bus or event map.
- Session loudness, measured in a play capture: -24 LUFS +-2 for TV and speakers, -18 LUFS +-2 for handhelds and
  laptops; true peak at most -1 dBTP; planned feedback stays inside about 10 LU, -28 to -18 momentary LUFS
  (godot-sound-design rule 11).
- Bad news is quieter, duller and shorter than the smallest reward. A higher reward tier is never quieter or shorter
  than a lower one.

## 7. Hand off

Deliver an asset pack with a manifest: per file its event, pool, take index, onset and peak, measured loudness and
peak, the variation rules for the engine, and the source (bank id and seed, recipe, or recording with its rights).
The `godot-sound-design` skill wires the files into the event-to-sound map and runs the in-game audit.
