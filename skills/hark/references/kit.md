# A film's sound kit: sfx and kit

This loop dresses a code-made film, or any cue sheet, with effects from the bundled bank instead of new synth code.
It loads no models. A full kit renders in about a minute; a re-render takes seconds. The `bank-kit-sfx` unit owns it.

## What the bank is

The bank's sounds come from code. The music-led families were tuned to match measurements of a private collection
of licensed reference clips, then checked for originality (CLAP similarity at most 0.84 to any reference). 15 more
sounds were fitted as code from CC0 recordings (`fit/<role>/<name>`, each naming its source); the presets use them for
clicks, ticks, keys, touches and the snap. Selection and checks used measurement only. Nobody ran a listening test.
None of these are recordings: a brief that asks for recorded sound goes to the `sound-sourcing` skill.

## Loop 5

1. Start from a preset: `hark kit init --preset <calm|crisp|minimal|launch> -o audio/kit.json`. calm: films led by
   text. crisp: energetic product films. minimal: designed effects only. launch: films edited to music. Each role has
   a `note` with the reason for its sound. calm, crisp and minimal switch off riser, swell and suck, so a reveal
   resolves on 1 element and the lockup is 1 clean hit. launch uses 1 riser or reverse swell at a section change plus
   the music-led families.
2. Adapt it. `hark sfx list --kind hit` (or `--category <family>`) lists candidates with lengths, peak times and knobs.
   Per role set `sound`; `edits` (`pitch` to the film's root for the hit or drop; `dur` and `apex` for a move of fixed
   length); `takes` (at least 12 for typing, 2-4 for other roles); `level_db`. Audition 1 candidate with
   `hark sfx render <ID> -o x.wav [--pitch E5] [--dur 0.8 --apex 0.32]` and read the onset, peak and end it measured.
3. Render: `hark kit render audio/kit.json --out audio/kit`. The mixer works from the takes and `kit.lock.json`
   (onset, peak, end and loudness per take). Placement: hits and touches put the onset on the cue frame; a whoosh puts
   its peak on the apex; a riser puts its end on the hit. A role that aligns on `peak` (the launch riser and swell)
   uses the peak, because its members climax before they end. `kit check` notes a role aligned on its end.
4. Check: `hark kit check audio/kit.json --out audio/kit`. It rejects settings no render can honour (an unsupported
   knob or pitch, a level above 0 dBFS, 2 takes writing 1 file). Each warning names a take and a cause: clicks, a
   truncated tail, comb ring, silence, pitch rising more than 4 semitones, a hit with 2 attacks or a build in front, or
   `pitch_off` (the knob missed the note). Replace the sound or change the knob; never keep a take with a warning.
   Read the notes too (a sounded riser, a composite sting).
5. Play the finished film through the review loop (`hark timeline --cues`, `hark sheet`) and confirm every cue landed.
   `hark kit audition audio/kit.json -o audition.wav` writes every take in a row at its role level.

## Music-led families (`hark sfx list --category <name>`)

- `ui_pop`: rounded mid pops for chips, bubbles and list rows coming in; the most used effect in such films.
- `knock`: wood and plastic in the mid band.
- `cut_hit`: short sub-bass hits with a kick.
- `mallet`: gated glockenspiel, celesta and marimba notes, a soft pluck, chime runs; `--pitch` tunes them.
- `drip`: water drops with a rising pitch (the check allows this rise for a bubble).
- `shutter`: snap zooms and whip-cuts.
- `reverse_swell`: peaks just after a cut; 1 per film.
- Also: big-whoosh thumps; riser dark, build and snatch; swish bright and flick; fast-whoosh whip; glitch burst, mid
  and chirp; notify mid-tine and mid-wood.

For films edited to music these beat bright 5 ms blips and long cinematic tails.

## Levels follow the music

With the launch preset, or any role that sets `bed_db`, effects sit 3-10 dB over the music bed and are heard through
timing and spectrum. A fixed level drowns in loud sections and pokes out of quiet ones. Per cue:

1. Render the kit. Each take stores its loudness; each role stores `bed_db` and `offset_ms`.
2. Measure the bed: the median 50 ms level of the music in the second before the cue.
3. Derive the take's gain from the lock, capped so the peak stays at or under -1 dBFS.
4. Place the take: taps, knocks and clicks lead the frame; whoosh peaks fall after it.
5. Keep a run of cues apart by at least the role's minimum gap (pops 114 ms, keys 87 ms).

The kit module has 1 helper per step 2-5. Before a hit, a drop, a big whoosh or a swell, dip the music 6-12 dB for
0.3-0.5 s and restore it on the hit (`mix.stop_down` in the lock). Do not duck for every effect. With no music bed,
`level_db` applies. When the music is silent just before a cue, the bed level is none and the gain falls back to
`level_db`. Report the gains applied and the bed levels measured.
