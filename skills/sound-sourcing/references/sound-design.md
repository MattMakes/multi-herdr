# Sound design craft rules

Values marked (norm) are measured norms of top-tier launch films. The other rules held on shipped films. The
launch-film reference profile itself belongs to hark's `timeline` unit (`hark critique`).

## The bar

- Every sound has a cause on screen, and its level matches the cause. Not every picture event gets a sound. Once 1
  class of action is sounded, every instance of it is sounded, at 1 level.
- Recorded and close: close-miked, dry, clean (floor under -55 dBFS between events), full bandwidth (95 % rolloff
  above 12 kHz), no clipping, no room of its own, no background events.
- 1 palette per film: 1 family of materials (felt and wood, glass and soft plastic, metal and air), 1 click voice, 1
  keyboard, 1 music track.
- The music drives the film. The effects punctuate it. Big moments and most cuts ride on events in the music.
- Restraint: about 5 designed effects per 10 s (norm, range 0-28). Put them where the music is sparse (cold open,
  typing, the logo build) and use fewer after the drop.

## Picture event to sound

| picture | sound | sits on | level and notes |
|---|---|---|---|
| cursor moves, hovers, scrolls, progress bars, streaming text, counters rolling | none | | continuous motion belongs to the music |
| click, tap, press | 1 recorded click voice for the film (mouse, trackpad, glass tap); press and release 60-110 ms apart, usually 85; release 0-3 dB softer (norm) | the press frame; pro films lead the visible press by 10-30 ms | about 3 dB under the music's short-term level, but 12-20 dB over it above 3 kHz (norm); bright and short (down 20 dB in 6-10 ms); centred; the same level every time |
| typing | 1 sliced recorded keyboard; space and return lower and longer | the frame each sounded character appears; at most about 12 keys/s; above about 25 chars/s, 1 key per word or silence (norm) | the music dips 6-10 dB under the run; vary the take and the level (sigma about 2 dB, never a jump over 6 dB), never the pitch; about 75 % of on-screen typing in pro films is silent (norm) |
| a card, chip, toast or badge appears | a soft recorded pop or tick, pitched to its size | 50 % of its travel | a run rises 1-3 semitones; pops that read like words sit 2-9 dB over the music (norm) |
| toggle, switch, lock-in, snap | a recorded latch, switch or magnet clack | the contact frame | short, within 1 frame |
| a counter reaches its value | 1 crisp tick on the final number | the frame it locks | about 8 dB under the music (norm) |
| camera-scale move, whip, zoom, logo zoom | a recorded swish (bamboo, stick, cloth, air past the mic) | apex on the velocity peak, onset 100-450 ms earlier (norm) | about 2 per film, never on every cut; may be wide; pans with the motion |
| hard cuts | none | | about 85 % of cuts in pro films carry no sound of their own (norm) |
| tension before the reveal | a recorded riser (soft-mallet cymbal roll, orchestral swell) or the music's own build | ends 30-250 ms before the hit, or stops on it | about 40 % of films use one (norm); never a crash cymbal on the hit |
| hero reveal | the music's DROP on that frame, optionally 1 recorded impact (drum, taiko, kick, a felt sub hit with a short top transient) | the contact or cut frame | the loudest point, at 30-60 % of the film, 2-7 LU over integrated (norm) |
| logo, lockup | the music stops on a bar line or is filtered down; 0.2-0.7 s of near silence; 1 soft element that resolves in the track's key | the first frame the wordmark is legible, never later | 8-20 dB under the film's loudest point (norm); tail 0.5-2.5 s, under -60 dBFS before silence |
| product objects | recorded foley of the object's material | the contacts | dry, centred |

## Music

- 1 licensed track at 110-130 BPM, or half time at 55-65. Shape: quiet intro, groove, a breakdown or stop at 40-80 %
  of the length, a drop for the reveal, then a resolve. Steady 4/4, mastered, full bandwidth, no vocals.
- Avoid: vocals, corporate ukulele and whistling, aggressive EDM, lo-fi hiss, trailer braams on a product film, dark
  moods for a friendly product, famous stock tracks.
- Brand personality rows: calm or literary: soft piano, felt, strings, no booms. Fast or exact: tight drums, plucks,
  a drop out of real silence. Bright or friendly: marimba, claps, pops, a major key. Precise or expensive: sparse
  piano, low strings, air. Raw or mechanical: ticks, latches, metal. Warm or kind: plucked strings, soft pads.
- Cut at bar lines (at 120 BPM in 4/4 a bar is 2 s). Splices keep the harmony going. The drop's downbeat sits on the
  hero frame. Stretch only when the tempo is more than about 0.2 % off, and never by more than about 6 %.
- Shape the track with rides, not with more music: down under typing, up into the drop, settled under the logo. Each
  ride step sits on a splice or under a transient, never inside a held note.

## Levels, arc, mix

- -14 LUFS integrated for web and social, true peak <= -1 dBTP after AAC (ceiling 0.77 gets there). -15 or -16 only
  with a written reason.
- LRA 5-9 LU for a film with an arc (norm; 1-2 LU sounds crushed). The first 2-3 s sit 5-12 LU under the body. The
  film starts on frame 0 with purpose: no fade over the first bar, no click on sample 0.
- The loudest momentary value is on the drop: not on the logo, not in the last 15 %.
- Effects dry, mono, within +-0.15 of centre. Width is for music, ambiences, risers and whooshes. Everything under
  about 120 Hz is mono.
- Each effect stands at least 3 dB over the bed in its best band; 6 dB is clear. A layer that adds nothing measurable
  wastes a placement.
- Nothing in 2-6 kHz goes far over the music's own loudest presence peaks: phones make that band painful first. An
  effect's spectral centroid stays under 6.5 kHz (norm). High pings at 9-14 kHz tire the ear.

## Sync

- Align `attack` for impacts, clicks, keys and pops on the contact, press or 50 % frame. Align `peak` for whooshes on
  the velocity peak. Align `end` for risers on the hit, or 30-250 ms before it. Align `start` for beds and crowds.
- A sound more than about 45 ms early arrives before its cause. A peak more than about 75 ms late is late (norm:
  accents land -45..+75 ms, median +8). Move the picture to the beat rather than pull a sound off its cause.
- A slow-attack sample aligned by attack can peak 80 ms after its first transient and arrive late.

## Variation without randomness

- A repeated action draws from at least 4 takes in `cycle`, with +-1-2 dB jitter and at most +-0.3-0.5 semitones.
  Real keys keep their pitch from press to press: vary the take, not the pitch.
- The UI click is the exception: 1 voice at 1 level.
- A progression rises: a pitch ramp over a run, a gain ramp into a build.
- Machine-gunning (1 sample, unvaried, in a regular rhythm) is one of the fastest ways to sound cheap.

## Tuning

- Anything pitched (a motif, celesta, bells, chimes) matches the track's key. Tune by resampling within +-2
  semitones. Prefer a recording near the right note over a big shift.
- Booms and subs sit on the track's root or have no pitch. An untuned boom under a resolving chord sounds cheap.

## Film foley

- Size comes from pitch and distance. A 1 cm character's steps are fingertip taps raised +2..+6 semitones, close and
  dry. A giant's steps are heavy foley lowered -3..-6 semitones with a longer tail.
- Build layers by job: body (weight at 100-400 Hz), transient (the short crisp contact at 2-6 kHz), detail (the
  material: a rattle, a creak, a rustle). 1 rule per layer on the same cue. `qa.py --layers` confirms each one adds
  something.
- Ambience zones: 1 bed per location, cut from clean windows of field recordings, changing with the picture's cuts and
  ducked under key actions with `auto`.
- Materials stay constant: 1 key sounds like that key every time.

## Choosing a recording

- Measure with `analyze.py` and the spectrogram. Listen with hark `describe` and `match` (quote the verdict words),
  `rank`, `find` (ready trims), `compare --refs`, `ab`, and `realness`, which must say "real or pro-produced" on the
  trim in use ("uncertain" needs its evidence recorded).
- Prefer lossless or HQ previews at 44.1/48 kHz or more, many takes of 1 object, and a clean floor between takes.
- Reject clipping, low bitrate (a cliff above 16 kHz), foley with room sound, voices or bumps in the needed window, and
  terms you cannot read on the item's own page.

`analyze.py` rules of thumb: foley and effects want a floor under -55 dBFS, 0 clipped samples, rolloff above 12 kHz
and no high-band cliff (16-19k far under 12-16k). An ambience wants a small level spread, side/mid above -12 dB (real
stereo), no spikes and at least 40 s usable. Music wants a steady tempo (small beat spread), a key, a per-bar loudness
arc and section boundaries as edit points.

## Cheap tells

- beeps, sine pings and noise sweeps made in code;
- 1 sample machine-gunned;
- a whoosh on every cut, or a boom on every logo;
- sparkle sweeps meaning "magic";
- reverb on small sounds;
- random pitch on typing;
- partial coverage: some presses heard, others not;
- clicks at cuts;
- music cut mid-note at a splice;
- an ambience loop you can hear;
- a different room on each sound;
- a logo louder than the drop;
- an ending that stops mid-note instead of decaying into silence.
