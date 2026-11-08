# bank-kit-sfx, part 5: sound-design rules for film effects

Part of the spec `docs/specs/hark/bank-kit-sfx.md`. Read the index first.

This part states, as rules, the sound-design knowledge that the bank, the kits
and the recipes depend on. The rules come from the source's two design notes (a
measured study of professional motion films, and a survey of how designers
build each kind of effect). We restate them in our own words, with the numbers
kept and the citations removed.

How to use this part:

- The recipe specs (recipes-impacts, recipes-motion) use sections 2-4 as the
  design brief for each recipe and its palette.
- The kit and preset work (part 3) uses sections 5 and 6 for placement and level.
- The audit (part 2) and the forge-core polish detectors use section 7 as the
  list of faults to measure.
- Each rule has a code (`G1`, `W3` ...) so other specs can point to it.
- A value marked "start" is a starting point for tuning, not a measured fact.
  Tune it by ear and with the hark measures (audit, polish, tone, envelope).
- A value marked "measured" was measured on professional films by the source.
  Those films are not available to us. Treat the numbers as targets, not as
  a test oracle.

## 1. How to measure (so that the numbers in this part mean one thing)

| code | rule |
|---|---|
| M1 | Measure an effect against its bed. Bed = the 150 ms with the lowest level inside the 0.6 s that precede the event. This choice keeps an earlier hit or a run of clicks out of the bed. A silent bed counts as -70 LUFS. |
| M2 | Level over bed (`over_bed`) = the loudest 100 ms of the event (K-weighted) minus the bed. The bed is a minimum, so this value reads some dB above a figure taken against the mean music level. |
| M3 | Band balance = for each third-octave band, the power that the event adds, as dB re the total power of the event. A band where the event adds less than 1 dB is masked and is left out. |
| M4 | Tilt = slope (least squares, dB/oct) of the band balance between 100 Hz and 10 kHz. |
| M5 | Attack = time to the peak, counted from the first 1 ms frame that is no more than 20 dB below it (envelope with the bed taken out). `t20` and `t40` = time from the peak to the first point 20 dB and 40 dB under it (10 ms smoothing). |
| M6 | Decay shape: `convex` = level stays up, then falls away (gate or fade); `exp` = level in dB falls on a straight line; `concave` = level falls quickly, then trails for a long time. |
| M7 | Pitch = the strongest spectral ridge, or the centroid for noise. When music sits under a sweep, read the sweep from a zoomed spectrogram, not from the automatic ridge. |
| M8 | Stereo: give the L/R correlation, and the side level relative to the mid level. "Pan travel" is how far the balance moves during the event. |
| M9 | The measured film numbers come from 58 hand-picked events in 8 films. One effects-only film gave 39 of them. Most category figures rest on 1-10 examples. They describe real professional work. They are not population statistics. |

## 2. General design rules (every effect)

### 2.1 What makes an effect "designed"

| code | rule |
|---|---|
| G1 | Use a physical model of a real object (a click, a key, a bubble) as the source layer only. Put a design stage over it to make a film effect: layers, a band for each layer, saturation, glue, space and a clean edit. |
| G2 | Build an effect from 2-4 layers that reach their peak at the same moment. For a hit: transient, body, sub, tail. For a whoosh: air, texture, a tonal layer, a landing. One layer alone sounds amateur. When a hit is flat, add a layer. Do not add more processing. |
| G3 | Give each layer its own band. Two body layers that share a band near 200 Hz cancel each other and make mud. Use 4 layers at most. |
| G4 | Anything that moves on screen must move in sound: filter, pitch, level and pan change together, along curves that speed up into the peak. A static spectrum with a volume fade sounds cheap. |
| G5 | Process and glue: a little saturation gives harmonics that survive small speakers; shape the transients; a bus compressor joins the layers without squashing them; one shared space; then a limiter. |
| G6 | Edit clean. Sample 1 of the file is the first sample of the transient. No silence comes before it. No hiss or room sound stays in the file unless the design wants it. Trim the tail but do not chop it. |
| G7 | Exaggerate. A sound pitched down sounds larger and heavier. A sound pitched up sounds smaller and lighter. Large moves are lower and slower. To get heavy sub material, take bright material and lower it by 2-3 octaves. |
| G8 | Work as a palette. Build all interface sounds of a product from a single patch. Vary only the pitch and the envelope. A family shares timbre, register and rhythm. Tune the tonal parts to the key of the music. |
| G9 | Sync to the frame and use sound sparingly. One or two frames off breaks the effect. Usually the music covers the motion, and the motion gets no effect. Large accents are rare. |
| G10 | Reference material for a designed effect must be designed library effects of the target style, not raw foley. A real-or-synthetic classifier gives high marks to mic hiss and room sound, and a designed effect has neither. |

### 2.2 The design chain for one sound

| step | code | what to do | start values |
|---|---|---|---|
| 1 | C1 | Band each layer with a high-pass, a low-pass and one static EQ move. | sub under 120 Hz; body 80-800 Hz; transient over 1-2 kHz; air over 2 kHz |
| 2 | C2 | Saturate the body and the transient a little. Never saturate the clean sub. Saturation creates new overtones; put EQ and compression after it. | tanh or tape curve, 3-9 dB of drive |
| 3 | C3 | Shape the transients: keep the crack, make its tail shorter. | |
| 4 | C4 | Glue the sum with a bus compressor. The attack must let the transient through. | ratio 2:1-4:1; attack 10-30 ms; release 60-150 ms; 2-4 dB of gain reduction |
| 5 | C5 | Space: reverb on the body and the tail only. Keep the sub dry. A pre-delay keeps the attack clear. Make the return darker. | pre-delay 10-30 ms; low-pass the return at 3-6 kHz |
| 6 | C6 | Stereo: keep the transient and the sub in the centre. Get width from the reverb and from decorrelated layers. The ear joins copies 2-50 ms apart into one sound; it does not hear an echo. Check that the mono sum still works. | |
| 7 | C7 | Finish: high-pass at 20-25 Hz, because energy under 25 Hz wastes headroom. Limit with look-ahead to -1 dBFS true peak. Match the loudness of the family. | |

### 2.3 Variation

| code | rule |
|---|---|
| V1 | A sound that repeats needs a few variants, chosen at random. |
| V2 | Typing never plays the same take twice in a row. |
| V3 | Per-take jitter (start): pitch +-0.3 to 1 st, level +-1 to 2 dB, timing and decay +-10 %, a new noise seed each time. |
| V4 | Keep the timbre fixed for a family, so that a set sounds like one product. In hark terms: a kit role keeps one sound and changes only the seed (part 3). |
| V5 | Make separate short, medium and long variants instead of time-stretching one file. Stretching smears the transients. |

### 2.4 What the professional films have in common (measured)

| code | rule |
|---|---|
| P1 | What counts most is where a sound goes and how few there are. Each sound makes one clear gesture (a move in pitch, a gate, a thump) and is cut to the picture. |
| P2 | In 6 of 8 films the music (and the voice) carries the cut rhythm. A cut has an onset within one frame only a few points more often than chance, and a strong accent within one frame only 4-25 % of the time. Designed effects come at a few chosen moments: opening interface ticks over a near-silent bed, tuned zips into cuts, a noise riser at the very end, a sub drop under a cut, an end chord. |
| P3 | The other model is a film scored only with effects: no beat, many drops to silence between ideas (12 in 30 s), a loudness range of about 12 LU, about -22 LUFS integrated. Every effect is exposed, short, band-limited and shaped. |
| P4 | One gesture each, mostly pitch or gate. Zips sweep at 20-60 octaves per second. Pop chirps fall at about 43 octaves per second. A sting chirp falls at about 38 octaves per second. Successive dings step up one semitone. A riser's tonal layer climbs 2 octaves. |
| P5 | Gate periods in risers and glitches are 20-45 ms. The period gets shorter near the cut (24 to 20 ms; 32 to 22 ms). |
| P6 | Each sound keeps to a band. Typing has no energy under 2 kHz. Pop-ins, taps, impacts and designed whooshes keep their energy under 150-250 Hz, and only a thin tick or air layer sits above, 20-40 dB lower. Dings have nothing under 500 Hz. Interface and transition sounds leave 300 Hz-1 kHz (voice and chords) to the music. Only stings, zips and chirps cross that band. |
| P7 | Ends are hard. Short interface sounds stay up and then stop (a convex decay: pop-ins 7/10, keys 5/6, dings 4/4). The two designed impacts and the chime run stop on a gate. A long natural tail is the exception (an end chord of the music). |
| P8 | Objects stay narrow and textures go wide. Pops, keys, clicks, whooshes and stings are mono or close to it. Shimmer and glitch textures have low L/R correlation (0.04-0.36) and travel (9-94 dB of pan travel). Pop-ins and the tails of impacts sit in the middle. |

## 3. Rules per effect: motion and transitions

### 3.1 Whoosh, swish, swipe

Layers:

| layer | job |
|---|---|
| air body | the motion: a band of filtered noise that moves (pink noise is smoother than white) |
| texture | material and distance: cloth, paper, brush; a cymbal swell makes the peak larger |
| tonal or style layer | a "digital" or "designed" identity. Optional: it gets repetitive when you use it too much. |
| low weight | size: a sine or hum for large moves |
| landing | the stop: its own hit, click or pop, used only when the picture shows the object stop |

| code | rule |
|---|---|
| W1 | All layers share one peak moment. Doppler: the pitch goes up as the object comes near and down after it passes. |
| W2 | Match the length to the move: a fast interface swipe or cut 0.25-0.5 s; a small card that crosses about a tenth of the screen about 180 ms; a title reveal about 0.48 s; a general whoosh about 0.8 s; a product fly-by about 1.15 s with its peak at about 54 % (620 ms); a heavy close pass about 1.2 s; a large reveal 1 s or more. |
| W3 | Envelope: quiet start, a fast build to the loudest point, then a trail. Mark three points: the motion starts, the fastest frame or the cut, the release. Place the peak at 50-70 % of the length. Shape the rise as an accelerating curve (an exponent of 2-3, or exponential in dB). Let the fall decay exponentially, roughly twice as fast as the rise (start). Loudness of a moving object grows roughly with the cube of its speed, which gives a short, crisp top. A short, steep rise reads as sudden. |
| W4 | Air whoosh (start): pink noise through 2-3 band-passes at fixed ratios (for example 1 : 1.6 : 2.5) that move together, one of them a broad wash. Q 1-3. The centre goes from 300-600 Hz up to 1.5-5 kHz at the peak, then back to 400-800 Hz. The centre follows the speed curve (centre proportional to speed to the power 0.5-1). |
| W5 | Heavy whoosh (start): centre 150 Hz-2 kHz, longer, plus a 40-80 Hz sine that grows with the peak, about 12 dB below the body. To get a cymbal-like peak, take the short reverb tail of a noise burst, reverse it, and end it on the peak. |
| W6 | Swish for a thin, fast object (a cane, a whip, a blade) (start): 80-250 ms, brighter (2-10 kHz), a steeper rise. Add a narrow whistle at f = 0.2 x speed / diameter (30 m/s past a 1 cm rod gives about 600 Hz) as band-passed noise with Q 15-40. |
| W7 | Digital interface swipe (start): 100-250 ms, noise high-passed at 300-800 Hz, and an optional small tonal chirp that glides up one octave across the swipe about 10 dB under the noise. No low end. |
| W8 | Doppler ratio over a pass = (c + v) / (c - v), c = 343 m/s: about 2 st in total at 20 m/s, about 5 st at 50 m/s. Use this ratio on the filter centre and on the tonal layer. In designed whooshes the tonal layer often gets a larger shift, 3-12 st. |
| W9 | Pan (start): equal-power left to right along tanh((t - t_peak) / w), with the width narrow, then wide, then narrow across the pass. The mono sum must stay intact. In hark, the seed sets the pan direction of a whoosh; the mixer flips the take left-right to match the travel. |
| W10 | Tail: a short reverb for size, or a pitched delay for a science-fiction feel. When a hit follows, leave about 80 ms of silence before it. |
| W11 | Processing: cut each layer's band, compress a little to join the layers, put a short room or plate on the tail, and add slight distortion to aggressive whooshes only. |
| W12 | In the mix: the peak is on the cut or on the frame with the fastest motion. The sound starts 0.2-0.6 s before. It sits under the picture and does not shout. Keep the longest, widest whoosh for the largest move. |
| W13 | Measured (effects-only film): a designed whoosh is a low swell, not air. Almost all of the energy is under 150 Hz (balance re total: 31 Hz -19, 63 Hz -14, 125 Hz -14, 250 Hz -19, 1 kHz -28, 4 kHz -31, 16 kHz -37 dB). Centroid 51-150 Hz, tilt -3 to -13 dB/oct, a faint air layer 20-30 dB down. Two whooshes under music were brighter (centroid 0.5-0.9 kHz). |
| W14 | Measured envelope: 0.5-1.0 s, symmetric or a little front-loaded; from -20 dB to the peak in about 190 ms, down 20 dB 90 ms after it and 40 dB 240 ms after it. The rise speeds up into the peak. |
| W15 | Measured motion: the centroid falls about 2.3 octaves during the pass. It is a pass-by, not a static noise. |
| W16 | Measured stereo: mono or almost mono (side 30-40 dB under mid), with no pan travel. Width in those films comes from textures, not from whooshes. |
| W17 | Measured timing: the peak is within about 150 ms of the cut, before or after, with the energy across it (one peaked 127 ms after its cut, one 313 ms before and died into it). Level over music: +15 to +21 dB. |

### 3.2 Riser (build, uplifter)

| code | rule |
|---|---|
| R1 | Layers: noise whose filter opens; a tone whose pitch climbs; an optional texture or grain layer, a rhythmic part (an LFO, tremolo or roll that speeds up), and a reverb that grows. 3-4 parts at most. One motion leads: the filter for a noise source, the pitch for a tonal source. |
| R2 | Noise layer: a 24 dB/oct low-pass or band-pass whose cutoff goes from 150 Hz to 10 kHz over the length, with a little resonance (Q about 1.5-3, start). It reaches its peak a little before the tonal layer. |
| R3 | Tonal layer: saw or triangle with 5-7 unison voices detuned 10-15 cents. The pitch climbs 12-24 st and narrows to 12 st near the peak. Stack layers at 0, +12 and -12 st that fade in at different times. The detune can widen during the rise. |
| R4 | Modulation that speeds up: LFO from a half note to a 16th note (1 Hz to 8 Hz at 120 BPM), depth from 10 % to 70 %. Rate and depth go up together. A frequency shift from 0 to +200 Hz gives an inharmonic lift. |
| R5 | Use exponential curves, not linear ones: most of the change lands at the end (the last beat or two, the last quarter). For example p(x) = p0 + (p1 - p0) x^3 with x = t / T, or exponential in log frequency. Level goes from -20 dB to 0 dB on the same curve. |
| R6 | An endless rise: sine (or saw) partials one octave apart under a raised-cosine loudness window over log frequency, all gliding up together. When a layer spans less than one octave, the listener hears it wrap around. Fade the top and bottom layers. |
| R7 | A converging reveal riser: some 30 voices begin on random pitches in 200-400 Hz and slide onto one chord that spans three octaves. It resolves into the logo chord. |
| R8 | Width grows into the peak. Reverb grows too: size from 30 % to 70 %; 2-3 s decay at 20-30 % wet for short risers, up to 6 s at 40 % for long ones. A reverse swell is a reverb tail played backwards so that it rises into the hit. |
| R9 | Length: 1-4 s in a launch film. |
| R10 | Ending: put the peak exactly on the downbeat or the frame of the reveal; a climax that comes late runs into the next section. After the peak, cut it off, or drop it 3-6 dB right before the drop, or insert 0.1-0.5 s (or one beat) of silence before the hit. The riser always gives way to a hit or a short silence. |
| R11 | Measured designed riser (0.84 s): three visible layers. (1) A stutter gate of 34 pulses at a median spacing of 22 ms (about 45 per second), getting faster from 24 ms to 20 ms toward the cut. (2) A tonal layer that climbs from about 2 kHz to 8 kHz (2 octaves; the last 0.22 s climbs 4.2 to 8.1 kHz, about 4 octaves per second). (3) Below everything, a constant low drone at 50-500 Hz. It stops on the cut (+20 ms), and a low whoosh or boom takes over and grows just after it. |
| R12 | Measured noise riser (1.15 s, end of a film): the band over 4 kHz climbs about 35 dB, the ridge goes up about 1.4 octaves (360 Hz to 1.1 kHz), the rise is concave in dB (slow start, fast end), side 14 dB under mid. A riser in the music: the high band rises 20 dB over 0.45 s, only +5 dB over the music. All three rises are concave: most of the rise is in the last third. Level over music: +15 dB. |

### 3.3 Downlifter (down-sweep, fall)

| code | rule |
|---|---|
| L1 | A downlifter is the mirror of a riser. Place it on the first beat of the next section, or right after a hit. It releases the energy of the build. Length 0.5-4 s (start). |
| L2 | Noise: the filter is open at the start and then closes: a low-pass from 8-12 kHz down to 200-400 Hz (start). A band-pass with more bandwidth and resonance also works. |
| L3 | Tonal: the pitch falls 12-24 st (start). For a pitch-down hit, put envelopes on both the pitch and the low-pass cutoff of a sine-square wave, so that the harmonics stay audible while the fundamental drops out of hearing. |
| L4 | Use curves that slow down: fast at first and slow at the end, an exponential with a time constant of about one third of the length (start). |
| L5 | The level falls while the reverb gets wetter, so the dry part dies first. An LFO whose rate follows a reversed envelope, with phase modulation and a flanger, gives the usual synth downlifter. |
| L6 | Tape-stop variant: the playback rate glides to 0 over 0.3-1 s. The steepness of the fall decides between a lazy sag and a hard pull. |
| L7 | Keep a downlifter 6-10 dB under the hit it follows, so that it does not mask it. All riser faults also apply. |

### 3.4 Glitch, stutter, digital

| code | rule |
|---|---|
| X1 | Bit depth: 8-12 bits for a light grit, 4-6 for a strong one (most useful: 6-10), 1 bit at the limit. Rate reduction: about 8 kHz sounds warm, about 2 kHz aliases hard. Band the signal to 500 Hz-2 kHz before the crush so that the lows are not damaged. Blend a strongly crushed copy at 20-30 % below the clean one. Low-pass or shelf the highs a little after the crush. |
| X2 | Buffer repeats: slices of 5-50 ms (start at 10-30 ms), each repeated 4-8 times. Make the spacing tighter for a ramp, keep it even for a stutter (1/32 to 1/64 notes). Step the repeat rate from 1/16 to 1/64 for a fill that speeds up. |
| X3 | Grains of 1-100 ms. 5-20 ms grains make obvious artefacts (begin with 20-30 ms). Short grains give a digital stutter; long grains give a smooth result. |
| X4 | Other sources of artefacts: very large time-stretches (4-8 times) of which you use the first 100-200 ms, reversed slices, random pitch jumps, cuts of odd length. Material with sharp attacks and a clear pitch keeps its identity when cut up; pads and noise turn into a blur. |
| X5 | Keep a steady sub or bass under a glitch so that the low end does not get thin. |
| X6 | Computed digital sources: robot-like chirps are tones on random steps with random glides (events every 100-700 ms; FM ratio 0-32, index 0-2). Data chirps (start): 5-30 ms bursts of square or pulse waves at 1-4 kHz, each burst on a different pitch. Electrical buzz: gated mains harmonics. |
| X7 | For motion (start): a glitch hit is short (100-400 ms) and locks to the visual glitch. Use one buffer slice for each glitched video frame (33 ms at 30 fps, 42 ms at 24 fps), so that picture and sound stutter together. |
| X8 | Measured (0.7 s, effects-only film): crackle first (sparse high grains at 4-12 kHz), then a dense full-band stutter of 11 bursts at a median of 28 ms (about 36 per second), getting faster from 32 ms to 22 ms into the cut. Bright (centroid about 9.2 kHz, tilt about +3.8 dB/oct, most energy in the 8 kHz band). The channels are unrelated (correlation 0.04; side level equals mid level; 9 dB of pan travel). It stops on the exact cut frame, where the sting begins. |
| X9 | hark's later glitch members (part 4, 3.1) put the loudest burst first: a glitch that hits, then speeds up. |

### 3.5 Shimmer, sparkle

| code | rule |
|---|---|
| S1 | Shimmer is decoration. Keep it for moments of magic (a generated result appearing, a "new" badge), and keep it short and quiet. Kinds: a broad, glassy band of highs; tiny bell notes placed at random; fine grains that drift rather than strike; one bright burst of glitter with a fast tail. |
| S2 | Ping cloud (start): grains of sine or FM bell, 15-60 per second, each a short ping. Take every pitch from a major or pentatonic scale in the music's key, between 2 and 8 kHz (C7 2093 Hz to C9 8372 Hz). Each ping: 0.5-1 ms attack, 30-200 ms exponential decay, a random pan, a random level within +-6 dB. Drive the grain rate with an envelope: more grains as the motion grows, fewer at the end. Pitches off the scale clash with the music. |
| S3 | Wash (start): noise high-passed at 6-8 kHz through a comb or flanger that moves slowly, about 18 dB under the pings. |
| S4 | Shimmer reverb: shift the reverb's wet signal up one octave and feed it back into the reverb input, so that each pass climbs again. The shifted signal goes only back into the reverb, never direct to the output; the send amount controls how strong it is. Put a low-pass in the loop, because the shift adds highs. |
| S5 | A science-fiction shimmer: a buzzy source into 12 band-passes (Q 1) whose centres move within 620-1200 Hz; noise into band-passes centred on 775, 1550 and 2325 Hz (Q 13) with a 10 ms rise and a 5 s fall; three moving taps on a 120 ms delay with 0.7 feedback; then reverb. |
| S6 | Processing: high-pass at 1-2 kHz, tame 6-10 kHz sibilance, a long airy reverb, width. Faults: tiring energy at 8-12 kHz; pitches outside the key; a grain rate that never changes; a shimmer that lasts too long or is too loud. |
| S7 | Measured grain sparkle (1.95 s): a tone held near 500 Hz under dense short grains in 1-10 kHz, one every 20 ms (50 per second); a 0.5 s swell, a fade of about 0.8 s. Balance: 500 Hz -11, 1 kHz -15, 2 kHz -9, 4 kHz -15, 8 kHz -20, 16 kHz -36 dB; tilt +6.5 dB/oct. Wide: correlation 0.08, side 0.3 dB under mid, pan travel 14 dB. |
| S8 | Measured chime run (1.95 s): glockenspiel-like notes (partials from 270 Hz to 10 kHz), every note panned hard to a different side (94 dB of pan travel); a gate stops the run. |

### 3.6 Zip (tuned sweep)

| code | rule |
|---|---|
| Z1 | Measured: zips are fast tuned sweeps (a sine-like glide plus harmonics), 0.18-0.28 s, only +7 to +9 dB over the music, which is lower than the hits. |
| Z2 | The sweeps are exponential: straight lines on a log-frequency axis, at a constant number of octaves per second. |
| Z3 | Measured shapes: up from about 100 Hz to 1.5 kHz in 0.2 s (about 20 octaves per second), starting 39 ms before a cut; up from about 100-150 Hz to 10 kHz in 0.22-0.25 s (about 30 octaves per second), ending on the cut (+20 ms); down from 7.2 kHz to 420 Hz in about 60 ms (about 60 octaves per second), across a cut with its peak 65 ms before it; an arc from about 1 kHz to 3 kHz and back over 0.25 s. |

## 4. Rules per effect: hits, interface and tonal

### 4.1 Impact, hit and braam

Layers:

| layer | job |
|---|---|
| transient | the timing: the sharp crack that the ear uses to place the hit. A short, bright burst, mostly over 1 kHz (snare, clap, stick, a noise burst). Make its tail short. |
| body | weight and character: kick, tom, large drum, a metal resonance, a gong |
| sub | punch you feel: a sine at 30-60 Hz with a pitch drop over 200-400 ms |
| tail | size and space: a long, dark reverb on the body that fades. The sub stays dry. |
| sheen or atmosphere | top and air: a processed cymbal, metal or an atmosphere |

| code | rule |
|---|---|
| I1 | Before the hit, pull back to almost silence: the contrast makes the slam. Or lead in with a reverse swell. |
| I2 | Transient (start): a noise burst of 2-10 ms with its envelope raised to the power 2-4 (for example 10 ms of noise with the envelope to the 4th), high-passed or band-passed over 1-2 kHz, plus a 1-3 ms click. 0 to -6 dB relative to the body peak. |
| I3 | Drum-like body (start): a sine or triangle that starts at 100-180 Hz and falls to 45-70 Hz with a pitch time constant of 30-80 ms. Add inharmonic membrane overtones at 1.59, 2.14 and 2.30 times the fundamental that die faster than it. Metal body: many inharmonic modes spread over 300 Hz-4 kHz; the higher the mode, the shorter its decay. |
| I4 | Sub (start): a sine that starts 12-24 st over its end note and falls exponentially over 200-400 ms. Tune the end note to the key of the music. A trailer or logo hit keeps the sub ringing for at least 800 ms. Game and interface hits get a few hundred ms. |
| I5 | A large explosive boom: a low part (noise into a 70 Hz band-pass and two 70 Hz low-passes; 100 ms attack; 8 s decay) plus a burst part (high-passed noise into a resonant low-pass that sweeps near 7 kHz in 30 ms, then a gain of about 50 into hard clipping). |
| I6 | Size: make a bright layer (noise or metal), then slow it to 1/4-1/8 speed, which is 2-3 octaves lower. This gives better sub material than pitching down a sound that is already low. |
| I7 | Tail (start): a 2-6 s reverb whose lows ring longest, darkened (low-pass 3-5 kHz), pre-delay 10-30 ms. A flanged tail (a comb with a moving delay) gives the falling fan of partials of trailer hits. |
| I8 | Processing: distortion on the body and the transient, then EQ and compression. Too much bus compression flattens the transient. High-pass under about 25 Hz. Pan the layers but keep the transient and the sub in the centre. Limit the sum. |
| I9 | Braam: a long, loud, low brass note at C1-C2 (about 33-65 Hz) with layers one octave apart. Vary each layer a little. Automate distortion on some layers, bend the pitch down slowly, and send the group to a reverb. Synthesis (start): for each octave layer, 3-6 detuned saws into a low-pass; the cutoff jumps from 200 Hz to 1.5-3 kHz within 50-150 ms (the brassy blat) and then rests near 800 Hz; drive that grows during the note; a bend of -1 to -3 st over 2-4 s; a sine sub under it. |
| I10 | In the mix: on the frame, and rare. The riser, drop-out, hit move happens once or twice in a film. Keep the largest sounds for the largest moments. |
| I11 | Measured: the sub dominates (balance re total: 31 Hz -15, 63 Hz -9, 125 Hz -12, 250 Hz -18, 500 Hz -36, 1 kHz -45 dB). Under 150 Hz holds all of the energy except about 0.7 dB. Tilt -3.6 to -5.7 dB/oct. Ridge at about 82 Hz. |
| I12 | Measured layers: a full-band transient at the onset, the sub body, and a tail with texture: a grainy low rumble (pulses every 28 ms, about 36 per second, slowing a little) for 1.2 s, or a high air band (5-10 kHz) that holds over the body. |
| I13 | Measured envelope: 50-290 ms from the onset to the loudest point (the hit swells into the body instead of peaking on the transient). `t40` 230 ms to 1.08 s; the rumble decays about 38 dB/s. Both designed impacts stop on a hard gate; the tail is cut off instead of ringing out. |
| I14 | Measured stereo: the body is in the centre. The rumble tail and the air tail have low L/R correlation (-0.01 and 0.24), a side about as loud as the mid, and 28-30 dB of pan travel. |
| I15 | Measured timing: the build begins 110-200 ms ahead of the cut, and the peak falls between 65 ms ahead of it and 82 ms after it. |

### 4.2 Sub drop and 808 boom

| code | rule |
|---|---|
| U1 | A sub drop is a sine that glides down into the sub range. Add a little distortion or harmonics so that it survives phones and laptops. A short drop supports a hit. A long descent means dread. It sits under a hit or a downbeat as its low base, with the other parts high-passed to make room. |
| U2 | Pitch (start): begin 1-2 octaves higher than the target; the target is 30-45 Hz, in the key. The glide is exponential in frequency over 0.4-3 s: a fixed number of semitones per second, or quicker at the start. |
| U3 | Level (start): a 2-5 ms attack ramp that starts at a zero crossing so that it does not click. Keep the level for about 30 % of the length, then decay exponentially. Finish with a fade; do not cut. |
| U4 | Small speakers reproduce little under about 300 Hz, but the ear infers a missing fundamental from its harmonics. So add a saturated copy in parallel (tanh, 6-12 dB of drive) (low-pass 200-400 Hz) beside the clean sine, or make the 2nd and 3rd harmonics directly at -12 to -20 dB. |
| U5 | 808-style values: the base sine at about 40-60 Hz; the pitch begins 1-2 st above and drops in 10-30 ms, which gives a clicky attack (a larger, longer fall gives a clear sweep); level about 5 ms attack, about 700 ms decay, no sustain, 400-500 ms release. |
| U6 | 808-style boom: a low sine (about C1-C2), the fastest attack, a long decay, no sustain. The pitch envelope decides kick or boom: a short pitch decay gives a kick, a long one gives a boom. Then saturation, mono, compression and a low-shelf cut. Do not add a sub-oscillator: its harmonics make the sub muddy. |
| U7 | Always mono. Only one sub sounds at a time in the whole mix. No reverb on a sub. No energy under 25 Hz. No clicks at the start or the end. The sub must be in key. |
| U8 | Measured: a sub tone from 64 Hz to 44 Hz (about half an octave over 0.75 s) that enters on a cut with a bright transient on top. Under 150 Hz holds all of the energy except about 0.5 dB. 430 ms to the sub peak (it grows after the cut: peak about 380 ms after it), decay about 42 dB/s (straight in dB), `t20` 360 ms. Narrow (side 13.5 dB under mid). +13 dB over voice and music. |

### 4.3 Interface click, tap, toggle, hover

| code | rule |
|---|---|
| K1 | Length: taps and clicks 30-150 ms; most interface cues under 300 ms; notifications up to about 500 ms. Clicks can be 10-80 ms. A click under about 100 ms, a confirm 100-250 ms. Cut cues shorter than seems natural alone, because they play often. |
| K2 | Meaning: neutral (tap, toggle, select) = one tone with no pitch move. Positive (confirm, success, forward) = pitch goes up, a major third or a fifth. Negative (error, cancel, back) = lower, falling, a little detuned or dissonant; a flat, dull tone means "refused". Do not invert these. |
| K3 | Toggle: "on" goes up or sits higher, "off" goes down or sits lower. A real toggle makes two contact sounds, 30-150 ms apart. |
| K4 | Hover: almost silent or nothing. When used: 30-50 ms at most, 12-20 dB under the click, nothing over about 4 kHz (start). |
| K5 | A frequent cue is higher, brighter and quieter. A rare cue is lower and fuller. Most interactions make no sound. Silence is the space that lets the important sounds stand out. |
| K6 | Plastic click: white noise through a band-pass at 5 kHz, Q 12. Envelope: rise for 1 ms, then fall for 20 ms from 2 ms on; square the envelope, then multiply the noise by it. As a reusable unit: inputs attack (ms), decay (ms) and centre (Hz), Q 8. |
| K7 | Push-button: clicks at 0 ms (1 ms up, 20 ms down, 4 kHz), 30 ms (1 / 50 ms, 3 kHz), 140 ms (4 kHz) and 150 ms (7 kHz), summed into a 50 ms delay line; its output passes a gain of 0.1 and a 700 Hz band-pass (Q 3) and feeds back, which gives a small resonant body. Slide switch: two pink-noise clunks, at 0 ms and 200 ms, and a brief ping made of sines at 10.5 kHz and 9.45 kHz that decays in 50 ms. |
| K8 | FM tick: carrier 1-10 kHz, modulator 100-1000 Hz, index 10-1000, attack 1-10 ms, release 1-10 ms, high-pass at 1 kHz. A fast envelope on pitch and on filter or level gives bells, pops, clicks and bleeps. |
| K9 | Tuned blip (the digital interface style): a sine or triangle at 600 Hz-2 kHz, 0.5-2 ms attack, 20-60 ms exponential decay; optionally a 1-3 st drop over 10-20 ms for a "plip". Put a short tactile texture under it (start): a 1-3 ms noise click high-passed at 3 kHz, 6-10 dB down. |
| K10 | Physical button: a strong shell mode at 2-7 kHz that damps out in 0.2-2 ms, over quieter modes; only a few housing modes in the mid band, each gone within a few ms (a housing that rings for long near 1 kHz turns the click into a wood block). The release follows 25-35 ms later at -25 to -30 dB, or is absent. |
| K11 | Processing: high-pass at about 120 Hz; tame harsh 2-5 kHz peaks; at most a very small, short room or plate; limit, then normalise so that all cues sound equally loud; mono unless there is a reason; cut the start to the first transient (silence before it makes the interface feel slow). |
| K12 | Palette: a single patch with one shared reverb; vary only pitch and envelope. Or make one confirm sound and derive the others by pitch, length and filter. Musical timbres with several harmonics work better than pure tones. Rounder, softer sounds alert without stress. |
| K13 | In the mix: one click for each click on screen, on the frame where the cursor presses. Short and bright (2-6 kHz), about 10-15 dB below the music peaks, panned to the side where it happens. |
| K14 | Measured bright interface clicks: attack 0-3 ms, `t20` 6-16 ms, centroid 5-10 kHz, tilt +4 to +12 dB/oct; one had tuned partials at 5-8 kHz (a "tink", not a noise snap). A mouse-like click over music: full-band with a low body (centroid 430 Hz, tilt -3.7 dB/oct), attack 10 ms, `t20` 11 ms, `t40` 37 ms, +13 dB over the music. Mostly mono (side 10-14 dB under mid). One opening click landed 31 ms before its cut. |
| K15 | Measured hover ticks: 35-80 ms windows, attack 0-5 ms, `t20` 6-25 ms, centroid 2.8-9.5 kHz, tilt +2.4 to +6.2 dB/oct; balance 1 kHz -23, 2 kHz -18, 4 kHz -13, 8 kHz -8 dB, almost nothing under 500 Hz. Two kinds: a tuned ping (one partial at 6-15 kHz, about 10 ms) and a filtered noise tick (2-12 kHz, 25-40 ms, its spectral peak moving down during the decay). +15 to +20 dB over a quiet bed. |
| K16 | Measured tap: a 150 Hz thump (ridge 130-160 Hz) with a faint full-band click, 0.1 s, attack 11 ms, `t20` 18 ms, `t40` 27 ms, tilt -6.5 dB/oct, side 5 dB under mid, 86 ms before its cut. |

### 4.4 Pop, bubble, pop-in

| code | rule |
|---|---|
| B1 | Bubble physics: a gas pocket rings at f = 3.26 / r (Hz, r in metres), 0.5-3.5 kHz for bubbles of a few millimetres. Damping gives Q 5-40. The pitch goes up as the pocket gets smaller and its neck opens: +10 % to +120 % in 3-80 ms. First comes the tick of the film breaking, then breathy noise that tracks the pitch, and a small low puff. |
| B2 | A simple bubble: a cosine whose frequency follows an exponential ramp (about 100 ms, or 200 ms) times a base frequency (about 600 Hz, or 240-270 Hz for a stream of bubbles); an attack-decay envelope of about 10 ms and 80 ms that starts 3 ms after the trigger; then a gain of 0.1 and a 40 Hz high-pass. A stream: a 15 ms clock, steps of prime numbers and a 50 % chance gate. |
| B3 | Motion pop-in (an icon, a bubble or a chat message appears) (start): a tuned blip that sweeps up from 300-500 Hz to 800-1500 Hz in 15-40 ms. Attack 0.5-1 ms, not 0 (a perfect step reads as a digital tick). Exponential decay 40-120 ms. A 1-2 ms band-passed noise tick (2-4 kHz) on the onset, 6-12 dB down. Optionally a second, smaller, higher ring 5-15 ms later (a daughter bubble) for a wetter, cuter pop. |
| B4 | When several items pop in, step the base pitch up a scale in the key of the music (pentatonic or major). |
| B5 | Edit: 0.2-0.4 s in total, 40 dB down 35-145 ms after the peak, little under 150 Hz or over 4 kHz. Professional kit pops last 0.19-0.41 s. Faults: tails that run long; a tick and hiss across the full band; a big low puff; one pitch for all elements. |
| B6 | Measured chirp pop: a sine that falls from 9.8 kHz to 350 Hz in 110 ms (about 4.8 octaves, about 43 octaves per second), with a 20 ms blip at 3 kHz before it. The loudest point is in the middle of the sweep (1-6 kHz); `t20` 14 ms after it. Measured box pop: a 1.2 kHz tone plus harmonics, switched on and off as a 70 ms box (2 ms attack, 7 ms release). Over music: a tuned blip from 1.1 kHz down to 100 Hz plus a click, 14 dB over the music. The three pops are mono. |
| B7 | Measured element pop-in (the effect used most in the effects-only film): one for each element that appears, fired in a run (17 events in 3.3 s, spacing 120-310 ms, median about 205 ms, settling into a steady 200 ms pulse). Its body is a thump at 80-150 Hz (ridge near 105 Hz; within 0.5 dB, all energy under 150 Hz; tilt -4 to -6 dB/oct). On top: a brief noise tick (1-10 kHz, about 30 ms, 30-40 dB below the body). Often the body hits twice, 60-70 ms apart. 0.14-0.27 s; attack 6-19 ms (median 16); `t20` 16-35 ms (median 20); `t40` 30-80 ms; convex in 7 of 10. Moderately wide (side 3.5-9 dB under mid, correlation 0.38-0.78). It lands 25-40 ms after its cut. |

### 4.5 Notification, ding, success chime

| code | rule |
|---|---|
| N1 | Success: two fast notes going up, a major third or a fifth. A notification is a short motif of 2-3 notes, up to about 500 ms. Error: lower, falling, a little detuned. A listener must tell success from error without looking. |
| N2 | Hierarchy: brand sounds, then rare celebration sounds, then alerts and notifications, then the primary and secondary interaction sounds. A notification fires often, so it is shorter and built to survive repetition. |
| N3 | Earcon limits: pitch between 125-150 Hz and 5 kHz. Notes no shorter than 0.0825 s (one- or two-note earcons can go down to 0.03 s). Up to 6 notes per second. A 0.1 s gap between earcons in a series. 10-20 dB over the background. When two earcons must be told apart absolutely, put them 2-3 octaves apart. |
| N4 | A brand sound can be two notes one octave apart, played a little apart (flammed), finished in under 1.5 s. Its key can anchor the whole palette. Rounder, softer alerts still notify without stress. |
| N5 | FM bell or chime: when the carrier-to-modulator ratio is not a whole number, the partials are inharmonic and metallic (a bell is about 1 : 1.4). A fast decay of the index makes the attack bright and the rest softer. The level decays as a long exponential. Start values: carrier = the note (for example C6 1047 Hz, E6, G6); ratio 1.4 (bell), 3.5 (glass) or 1 or 2 with about 0.2 % detune (electric piano, soft chime); the index falls from 3-8 to about 0.5 with a time constant of 30-80 ms; level time constant 150-600 ms. |
| N6 | Modal bell. Groups of partials, each as frequency ratios to the fundamental, then strengths, then a decay factor (decay = 2000 ms x factor): main 2.002, 3, 9.6 (0.008, 0.02, 0.004; x0.9); second 2.49, 2.571, 11 (0.02, 0.04, 0.02; x0.25); low 0.501, 1, 0.7 (0.002, 0.001, 0.02; x1.2); circular 3.05, 6.242, 12.49 (0.005, 0.05, 0.05; x0.14); striker 13, 16, 24 (0.02, 0.03, 0.04; x0.07). The striker is noise with a 10 ms decay and its envelope to the 4th. For a single ding use one bell (a phone ring uses two at 650 and 653 Hz, struck in turn every 60 ms). The casing: two short feedback delays (0.77 and 0.88 ms, gain 0.7) and band-passes at 287, 431 and 1243 Hz (Q 12). Higher partials die faster, and that is what makes it sound like struck metal. |
| N7 | Glass or marimba ding (start): the modes of a free bar at 1 : 2.756 : 5.404 : 8.933, each partial decay falling roughly as f^-0.9 (tines follow the same law). Add a soft 2-5 ms mallet (a low-passed noise pulse). |
| N8 | Processing: high-pass at about 120 Hz, a very small plate or room, a little width, a limiter. Tune the notes to the key of the music. |
| N9 | Faults: alarm-like beeps (alarms are plain sines at about 600-1012 Hz switched 2-4 times per second), square waves, a frequent cue with a tail longer than about 0.6 s, success and error that sound alike, pitches below 125 Hz or above 5 kHz. |
| N10 | Measured dings: a struck tone plus a single inharmonic partial (fundamental near 1.8-2.2 kHz, a partial near 3.4 times it, about 6.3 kHz); attack 1-15 ms, `t20` 27-42 ms, `t40` 30-180 ms (short and convex: they ring a little and stop); 0.23-0.36 s in total. Successive dings go up one semitone (1822, 1934, 2045, 2168 Hz; ratios about 1.06). Balance: 1 kHz -6, 2 kHz -5 dB, nothing under 500 Hz (under -38 dB). Moderate stereo (side 5.5-13 dB under mid). +16 to +17 dB over their bed. The `pro_notify` members follow this (part 4). |

### 4.6 Typing

| code | rule |
|---|---|
| T1 | One key press: the press lasts about 10 ms and has two peaks: first the finger meets the cap, then the cap meets the plate. A much weaker release peak follows about 100 ms later. On a laptop keyboard the release can be missing. |
| T2 | A mechanical key, as measured on real keys: a full-band bottom-out click; cap and plate modes near 1.25-1.6 kHz with a cluster up to about 2.3 times; a body from the case at 110-260 Hz; a thock at 58-77 Hz with its peak 20-35 ms after the hit; then, 70-125 ms later, the release (two small ticks and then the key reaching the top); an 8-10 kHz spring ping. |
| T3 | Timing: one key sound for each typed character, never the same take twice in a row, a little irregular. People type 40-80 words per minute, about 3-7 characters per second (150-300 ms apart). On-screen typing in launch films is often quicker, 10-20 characters per second (50-100 ms apart), and each release then overlaps the next press. Vary each interval by +-15-25 %. Make the gap a little longer before a space and after punctuation (start). |
| T4 | Synthesis (start): a fixed set of modes for each key (one key sounds the same twice) plus variation for each press: force +-2 dB, timing +-10-20 ms, mode frequencies +-1-2 %. |
| T5 | Space bar and Return are big keys: cap modes at 0.6-0.8 times the normal key, a stabiliser rattle (2-4 small ticks in 5-15 ms), a louder release. |
| T6 | Laptop and scissor keys (the usual sound of an interface demo): quieter and shorter, without a thock, brighter (2-6 kHz), with a weaker release, and a high-pass at 120-200 Hz. A mechanical thock is a style choice, not the default. |
| T7 | Stereo: a small pan by key position (left-hand keys to the left), within +-20 %. Faults: a machine-gun effect (the same take or perfectly even timing), a large low thock under a light interface demo, characters and sounds out of sync. |
| T8 | Measured designed typing: high-passed noise only. Each key is a noise burst at 3-12 kHz (ridge 8.6-11 kHz) with almost nothing under 2 kHz (bands to 1 kHz at the -60 dB floor; 2 kHz -28, 4 kHz -13, 8 kHz -7, 16 kHz -14 dB re total); tilt +12 to +19 dB/oct; attack 2-5 ms, `t20` 11-18 ms, `t40` about 60 ms; convex; mono. |
| T9 | Measured typed run (1.55 s): keys 62-148 ms apart (median about 87 ms, about 11.5 keys per second), with jitter in level and timing; near the end of the run a soft, low swell rises beneath it. A key in a voice-over gap was a more realistic mid-range key (centroid 3.2 kHz). |

### 4.7 Logo sting, reveal

| code | rule |
|---|---|
| O1 | Shape: a build (riser or reverse swell, 0.5-2 s), then a drop-out (0.1-0.5 s of near silence), then on the logo frame a hit (an impact with its sub), then a 2-4 s tuned tail or bloom that brings the music to rest. |
| O2 | Known patterns: a small object knocked on wood or metal, layered, followed at once by a bloom from an effects-laden instrument (tension, release, a little quirk); two piano notes one octave apart, flammed, in under 1.5 s; voices that converge from random pitches onto a wide chord. Sonic logos are usually 2-5 s, most often 3 s. |
| O3 | Hit (start): use the impact recipe, with a sub of 0.8-1.5 s only. |
| O4 | Tonal bloom (start): FM or modal bells and a pad playing a chord in the music's key (root, fifth, octave; a major third or a ninth if wanted). It comes in 0-30 ms after the hit, rises over 50-200 ms, and its low-pass opens from about 1 kHz to 6-8 kHz. Shimmer reverb on the bloom only. |
| O5 | Optional lead-in: a reverse swell of the hit's own reverb for 0.3-1 s into the hit, or a converging riser that ends on the bloom chord. |
| O6 | Timing: the hit lands on the frame where the logo lands. End no more than about 0.5 s after the last frame. In 5 of 7 professional films, the largest hit in the final 4 s rings for 2.0-3.6 s. |
| O7 | Faults: a slow fade or empty silence to finish; an over-long riser; a tuned tail in the wrong key; all parts at the same moment without a drop-out; a sub tail that lasts longer than the picture. |
| O8 | Measured designed sting: a chirp that falls from 1.5 kHz to 150 Hz in 85 ms (about 3.3 octaves, about 38 octaves per second) onto a low tuned hum that rings about 0.8 s (fundamental near 64-80 Hz, harmonics up to 160 Hz, the pitch sinking about 0.6 octave). Mono (correlation 1.0). Balance peaks at 250-500 Hz (-13 and -6 dB re total), tilt about -6.9 dB/oct, `t20` 58 ms, `t40` 260 ms, a concave tail. It begins 28 ms after the cut on which the previous glitch stops. A musical ending: a final struck chord ringing for 4 s (centroid 1.9 kHz, decaying about 130 dB/s at first, then the tail). |

## 5. Placement against the picture

| code | rule |
|---|---|
| PL1 | For impacts, sub drops, taps and clicks, the attack begins 30-200 ms ahead of the cut. The body or the peak lands within about 80 ms of the cut. A sub can grow up to about 380 ms after it. |
| PL2 | Whoosh peaks land within about 150 ms of the cut, before or after. |
| PL3 | Risers, rising zips into a cut, and glitches stop on the cut (0-20 ms late at most), and the next sound takes over at that point. |
| PL4 | A pop-in comes 25-40 ms after its cut. |
| PL5 | Between ideas, leave silence or leave the music alone. Silence before a large moment is part of the sound. |
| PL6 | Sync tolerance: viewers notice sound about 45 ms early or about 125 ms late. Broadcast practice allows +40 ms / -60 ms end to end. A frame at 30 fps is 33 ms. Align the heard onset or peak to the exact frame. If unsure, put the sound early by one frame at most. Never let it lag by two frames or more. Align takes on their attack, not on the file start. In hark this is the kit `align` field and `offset_ms` (part 3). |

## 6. Levels and mixing in a film

| code | rule |
|---|---|
| E1 | Music leads, and effects are accents. Measured: all salient hits together (music included) come at 1.2-1.7 per second, and cuts do not each get a hit. Designed accents are kept for the reveal, the logo, the main interface action and changes of section. |
| E2 | Final mix: about -14 LUFS integrated; true peak no higher than -1 dBTP. |
| E3 | Drop-outs: 4 of 7 films drop 10 dB or more for 0.15-1.5 s and return with a hit. |
| E4 | Interface foley sits about 10-15 dB under the music peaks, in the 2-6 kHz band, panned to where it is on screen. Hits are the loudest effects. Whooshes sit under the picture. |
| E5 | At any moment only one part has the sub; high-pass all others. Phones play mono, so test the mono sum. |
| E6 | Over music (measured with M2): hits, clicks, pops and dings are 13-17 dB above the quiet bed between the beats (up to +21 dB for a whoosh). Tuned zips sit +7 to +9 dB. In an effects-only film the effects peak at about -16 to -38 LUFS (loudest 100 ms) with silence between the ideas. hark expresses this with the kit `bed_db` key (part 3, 6.1). |

## 7. The cheap-effect checklist

Each item is a fault that a recipe check or the audit can measure. The column
"hark check" names where hark measures it, when it does.

| code | fault | hark check |
|---|---|---|
| F1 | One layer only, or layers whose peaks do not line up. | the attack-layer detector finds stacked onsets (part 2, 3.7) |
| F2 | No movement: a static spectrum and a static pan on something that moves. | none (recipe review) |
| F3 | Linear curves on risers and sweeps where an exponential or accelerating curve belongs. | none (recipe review) |
| F4 | Mud: too much energy at 200-400 Hz, or two layers that fight in one band. | none (tone measure, recipe review) |
| F5 | A soft or smeared attack: too much bus compression, or a slow start on a sound that needs a crack. | attack measure |
| F6 | Too long: interface cues over about 150 ms, notifications over about 500 ms, tails that run into the next event. | the `tail` and live-time rules (part 2) |
| F7 | Bad low end: energy under 25 Hz; a pure-sine sub with no harmonics; a sub out of key; reverb on the sub. | DC rule only |
| F8 | Harshness: unfiltered white noise, 2-5 kHz peaks on interface sounds, crush and aliasing stacked. A naive oscillator folds over. | forge-core polish (aliasing) |
| F9 | Artefacts: clicks at the edges or at splices, DC, chopped tails, silence before the start. | `clicks`, `edge`, `dc`, `truncated`, `silence` (part 2) |
| F10 | Inconsistency: a family at different loudness or timbre; pitches not in the key of the music. | kit level (part 3) and the `pitch_off` rule |
| F11 | Too much sound: one whoosh or riser repeated on each cut, a sound for each tiny motion, hover sounds. | `kit check` notes (part 3, 5.6) |
| F12 | The wrong size or texture for the object: a large object needs a lower, slower, wider sound; a small one a higher, faster, narrower sound. | none (listening) |
| F13 | Off sync by one frame or more. | none (timeline) |
| F14 | Width that disappears in mono on phones. | correlation measure (part 3, 6.4) |
| F15 | Judged alone only. Listen against the music and on small speakers. | `kit audition` (part 3, 5.7) |

## 8. What the recipes must offer (requirements for the recipe specs)

| code | rule |
|---|---|
| RQ1 | Every motion recipe has a design stage: the physical model stays as a source layer, plus the other layers of its category (sections 3 and 4) and the chain of 2.2. Parameters to expose: a level per layer, saturation drive, glue amount, reverb send, pre-delay, and edit length. |
| RQ2 | Sync parameters: for whoosh, riser, downlifter and sub drop the length and the peak time; for impact and sting the frame of the hit; for anything tuned or with a sub, a note or key. A film can then ask for "a 0.48 s whoosh with its peak at 0.30 s in D minor". In hark the bank exposes these through the `dur`, `apex` and `pitch` knobs (part 2, 4). |
| RQ3 | Family parameters: a family seed fixes the timbre (mode set, FM ratio, filter character) of a set, and the take seed varies only force, timing and noise. This gives a consistent interface palette and safe round robins. |
| RQ4 | Measurable targets for checks: where the peak sits as a fraction of the length (0.5-0.7 for a whoosh); curve shape (most of a riser's change in the last 25 %); band shares (interface: little under 120-150 Hz; pops: little under 150 Hz or over 4 kHz; impact: energy in each of the sub, body and transient bands); click attack under 2 ms to the peak; length per category (8.1); earcon pitch inside 125 Hz-5 kHz; loudness spread across a family; loss in the mono sum. |

### 8.1 Target table per category (measured on professional films)

A recipe reads as "professionally designed" in this style when its render,
measured alone (bed = silence) with the measures of section 1, falls in these
ranges.

| category | length | stereo | balance | attack | tail | pitch or motion |
| --- | --- | --- | --- | --- | --- | --- |
| whoosh_big (designed swell) | 0.5-1.0 s | mono | 80 % or more of the energy under 150 Hz; tilt -3 to -12 dB/oct; an air layer 20-30 dB down | -20 dB to peak in 150-250 ms, speeding up | -20 dB in 80-120 ms, -40 dB in about 250 ms | centroid falls about 2 octaves during the pass |
| riser | 0.8-1.5 s (longer allowed) | mostly centre (side about 14-18 dB down) | high noise, a tonal layer and a drone | accelerating; the last third holds half the rise | stops on the hit or cut; the next sound takes over | tonal layer +2 octaves (2 to 8 kHz); gate from 24 to 20 ms (40-50 pulses per second), speeding up; high band +30 to 35 dB over the build |
| impact | 0.6-1.3 s | body centred, tail decorrelated | over 75 % under 150 Hz; tilt -4 to -6 dB/oct; ridge about 80 Hz | a transient first; body peak 50-290 ms after it | a textured tail (grains every 25-35 ms), then a hard gate; `t40` 0.25-1.1 s | the tail grains slow down a little |
| sub_drop | 0.8-1.5 s | narrow | over 85 % under 150 Hz | a 30-50 ms transient; the sub grows 0.3-0.4 s after | about 40 dB/s, straight in dB | 64 to 44 Hz (half an octave over 0.75 s) |
| logo_sting (designed) | 0.8-1.5 s | mono | peak at 250-500 Hz; tilt about -7 dB/oct | a 70 ms chirp | `t40` about 260 ms, concave | chirp 1.5 kHz to 150 Hz in 85 ms into a hum at 64-80 Hz that sinks 0.6 octave |
| glitch | 0.4-0.8 s | fully decorrelated | bright: centroid about 9 kHz; tilt about +4 dB/oct | crackle first, stutter after | stops on the cut | bursts every 32 ms down to 22 ms (speeding up) |
| shimmer | 1.5-2.5 s | wide (correlation under 0.4), moving | 0.5-8 kHz; tilt +2 to +6.5 dB/oct | a 0.5 s swell | a 0.8 s fade or a gate | one held tone under grains 15-25 ms apart |
| notify | 0.2-0.4 s | almost mono | fundamental 1.8-2.2 kHz + an inharmonic partial near 3.4 times; nothing under 500 Hz | 1-15 ms | `t20` 25-45 ms, convex (it stops) | a series goes up one semitone per ding |
| pop (chirp) | 0.1-0.15 s | mono | mid-high | peak inside the sweep | `t20` about 15 ms after it | a sine that falls about 5 octaves in 110 ms (about 43 octaves per second); an optional 20 ms blip before it |
| pop (box) | 50-80 ms | mono | a 1-1.5 kHz tone + harmonics | 2 ms | a 5-10 ms release (box) | none |
| pop_in | 0.14-0.25 s | a little wide (correlation 0.4-0.8) | thump at 80-150 Hz, over 85 % under 150 Hz; a 1-10 kHz tick 30-40 dB down | 6-20 ms | `t20` about 20 ms; `t40` 30-80 ms; convex | may hit twice, 60-70 ms apart; in runs about 200 ms apart |
| tap | about 0.1 s | almost mono | a 130-160 Hz thump + a faint click; tilt about -6.5 dB/oct | about 10 ms | `t20` about 18 ms; `t40` about 27 ms | none |
| click (interface) | 30-60 ms | mono | centroid 5-10 kHz; tilt +4 to +12 dB/oct; tuned partials at 5-8 kHz allowed | 0-3 ms | `t20` 6-16 ms | none |
| hover_tick | 20-40 ms | mono | centroid 3-9.5 kHz; nothing under 500 Hz | 0-5 ms | `t20` 6-25 ms | either one 6-15 kHz ping or a noise tick whose peak falls |
| typing_key (designed) | 50-80 ms | mono | noise at 3-12 kHz only: under 2 kHz at least 30 dB down; tilt +12 to +19 dB/oct | 2-5 ms | `t20` 11-18 ms; `t40` about 60 ms; convex | none; in a run, spacing (60-150 ms, about 11 keys/s) and level vary |
| zip | 0.15-0.3 s | almost mono | a tuned glide + a few harmonics | 20-35 ms | stops on the cut | an exponential sweep at 20-60 octaves per second: up from about 100 Hz to 1.5-10 kHz, down from about 7 kHz to 400 Hz, or an arc |

Categories that the films did not show exposed: `whoosh_fast`, `swish`,
`swipe`, `toggle` and `downlifter`. Their targets come from sections 3 and 4
only.
