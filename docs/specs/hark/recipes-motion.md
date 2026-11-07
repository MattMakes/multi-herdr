# recipes-motion: the motion, sweep, glitch and tonal recipes of hark (index)

This unit is 15 recipe modules of the hark forge and their seed banks. Each recipe synthesises one family of designed
sound effects for motion design and video edits from a parameter dict and a random generator. The recipes use no
recordings: all output is synthesised.

The package is `hark`. The recipes are modules `hark.forge.recipes.<name>` (engine root `skills/hark/engine/hark/`).
The seed banks are `hark/forge/seeds/<name>.json`. The 15 names are whoosh, swish, swipe, brightswish, riser,
downlifter, zip, darkriser, revcymbal, glitch, burstglitch, notify, shimmer, hover and sting. The module names are
part of the public interface: the forge finds a recipe's seed bank by its module name, and render blocks name the
recipe (forge-core part 6).

The spec is above 60 KB, so it is split into an index and 5 parts. Read this index first. It holds the contract
that all recipes share, the shared primitives P1 to P26 that the parts cite, and the unit-level sections (Data
assets, Errors and edge cases, Acceptance tests, Dependencies on other units, Open questions). The parts hold the
per-recipe sound, Interface, parameter tables, presets, algorithms and seed-bank facts.

## Parts

| part | file | sections | recipes |
|---|---|---|---|
| 1 | [recipes-motion-pass.md](recipes-motion-pass.md) | 1 to 4 | whoosh (the pass engine, version 2 and version 1), swish, swipe, brightswish |
| 2 | [recipes-motion-sweep.md](recipes-motion-sweep.md) | 5 to 9 | riser (rounds 1 to 4 and the round-6 build), downlifter, zip, darkriser, revcymbal |
| 3 | [recipes-motion-glitch.md](recipes-motion-glitch.md) | 10, 11 | glitch (version 2 and version 6), burstglitch |
| 4 | [recipes-motion-tonal.md](recipes-motion-tonal.md) | 12 to 14 | notify (ding, success, bell, chime), shimmer (sparkle, run, chimes), hover |
| 5 | [recipes-motion-sting.md](recipes-motion-sting.md) | 15 | sting (chirp, reveal, launch and gesture idioms) |

Source coverage. Every source file of the unit maps to a section:

| source file | section | seed bank |
|---|---|---|
| whoosh | 1 | whoosh.json (1.x and index Data assets) |
| swish | 2 | swish.json |
| swipe | 3 | swipe.json |
| brightswish | 4 | none |
| riser | 5 | riser.json |
| downlifter | 6 | downlifter.json |
| zip | 7 | zip.json |
| darkriser | 8 | none |
| revcymbal | 9 | none |
| glitch | 10 | glitch.json |
| burstglitch | 11 | none |
| notify | 12 | notify.json (12.13) |
| shimmer | 13 | shimmer.json (13.9) |
| hover | 14 | hover.json (14.7) |
| sting | 15 | sting.json (15.12) |

## Interface

### The recipe contract

Every recipe follows the forge-core recipe contract (forge-core part 1, "The recipe contract"). The attributes that
the forge reads:

| attribute | meaning |
|---|---|
| `SPACE` | dict name → spec. A list is a set of choices; a one-element list pins the value. `(lo, hi)` is a linear float range, `(lo, hi, 'log')` a log range (lo > 0), `(lo, hi, 'int')` an inclusive integer range. |
| `DEFAULTS` | dict name → value: a complete, good parameter set. |
| `TARGET` | the text the critic aims for. |
| `ALTS` | list of confusion texts that the sound must match less than the target. |
| `REFSET` | (optional) the reference-set category for the critic. |
| `TARGETS` | (optional) dict category → target text, for a recipe that serves several categories. |
| `render(p, rng)` | p: a parameter dict; rng: a `numpy.random.Generator`. Returns float64 audio at 48 000 Hz. All 15 recipes return stereo, shape (n, 2). |

Family presets (FAMILY, FAMILIES, FAMILIES_BY_CAT, BASES and PRESETS, FAMILY6 and the round copies) are data that
drivers and the bank read; the forge does not read them itself. Their shape differs by recipe and is stated in each
part's Interface. The bank palette reads `FAMILIES` in the form {category: [(name, params), ...]} and calls
`preset_full(cat, params)` when the recipe has it (brightswish), so that members stored as overrides become full
sets (unit bank-kit-sfx).

Optional hooks that some recipes give the critic or drivers: `measure(x)`, `target_check(x, row=None)`,
`study_extra(x, row=None)`, `generic_extra(x, row=None)`, `extra_for(cat, weight)`, `pro_penalty(x, cat)`,
`family_space(...)`, `family_params(...)`, `local_space(...)`, `space_for(cat)`, `recipe(...)`, `recipe_for(cat,
preset)`. A recipe object made by `recipe(...)` or `recipe_for(...)` carries the module name of its recipe, so the
forge uses that recipe's seed bank.

### The forge-core render wrapper

The forge never plays a recipe's raw output. The forge-core `render(recipe, params, seed)` (forge-core part 7):

1. builds the generator as `numpy.random.default_rng(seed & 0xFFFFFFFF)`;
2. calls `recipe.render(params, rng)`;
3. returns None for an empty output, a non-finite value, or an absolute peak under 1e-7;
4. otherwise normalises the peak to -1 dBFS.

So a recipe's own output level only matters inside the recipe. Several recipes normalise to an absolute peak of
0.89; others leave the level as it falls out. Both are correct.

### Units and notation used by all parts

- SR = 48 000 Hz. t = sample index / SR.
- Times are seconds, frequencies Hz, levels dB, unless a key name says ms.
- "samples of s" means P1(s).
- Mono is (n,); stereo is (n, 2), column 0 left, column 1 right.

## Behaviour

These rules hold for all 15 recipes. Each part states where a recipe differs.

1. **Pure synthesis.** A render reads no file. It uses only its parameters, the generator and the shared primitives.
2. **Parameter merge.** Most recipes overlay p on DEFAULTS (or on a base set) before rendering, so a partial set
   renders. The exceptions are stated in their parts: the whoosh family overlays the OPTIONAL off values (1.2, 2.2);
   hover does not merge at all and needs a full set (14.2); sting overlays p on the base of its mode, not on DEFAULTS
   (15.7).
3. **Off values.** Optional layers have an "off" test (for example a level ≤ -59 dB, or a share ≤ 0.01). A layer that
   is off adds nothing. Keys added in later rounds default to off, so older parameter sets render as they did.
4. **Versions.** Old parameter sets keep their own render paths: whoosh version 1 (a set without `rise`), riser
   rounds 1 to 4 versus the round-6 build (`r6`), glitch version 2 versus version 6 (`roll0`), shimmer round 2
   (`r2_path`), the 4 sting modes, the 4 notify styles. The seed banks hold sets of every version, so a clean build
   keeps every path.
5. **Random draws.** The render generator supplies every random value. The glitch recipes also use a pattern
   generator seeded from the `pattern` key (part 3). The parts name each random quantity and its distribution. A
   clean build need not repeat the source's draw order, so its renders will not match the source sample for sample.
   It must be deterministic: the same p and the same seed give the same output.
6. **Edges.** Every recipe starts and ends its output smoothly: a fade-in of 0.3 to 30 ms, and a fade, a release or a
   gate at the end. The "hard end" of the riser and glitch recipes is a fade of a few ms that lands on the cut.
7. **Families and delivery.** Each category has hand-designed members. The forge tuned some of them with the
   character keys pinned (`family_space`, `local_space`, `narrow`) and delivered the result. Some rounds applied
   edits after a run (R4_EDITS). A part marks each family as delivered or not. The seed bank holds the delivered
   members as pinned entries.
8. **Knobs.** The forge-core knobs re-render a sound by changing named keys: length keys (length, dur, g_length), pitch
   keys (f0, tf0, key), the whoosh level curve (duration shape and apex), and for notify the decay (`ring`), onset
   (`soft`), brightness (`hard`, bell style) and width keys (forge-core part 5). The recipes must keep those keys and
   their meaning.

## Algorithms: the shared primitives

The parts cite these primitives by number. P2, P3, P9, P10, P11, P13, P14, P18, P19, P21, P23, P24 and P25 are
forge-core functions (forge-core part 2 holds their full algorithms); this table gives what the recipes rely on. The
others are recipe-level helpers, defined here in full.

| # | name | definition |
|---|---|---|
| P1 | sample count | n = max(1, round(s × 48000)). Every duration becomes samples this way, so a duration of 0 gives 1 sample. t_of(n) = (0 .. n - 1)/48000. |
| P2 | state-variable filter | forge-core `svf(x, cutoff, q, mode)`: a time-varying TPT state-variable filter; cutoff and q may change per sample; the cutoff is clipped to 20 Hz .. 0.45 × SR. Modes: low and high (gain 1 in the pass band; for q > 0.707 the response rises to roughly q at the cutoff), band (peak gain q at the cutoff), bandn (band-pass with peak gain 1). |
| P3 | Butterworth | forge-core `butter(x, kind, f, order)`: low, high or band (f = (lo, hi)), as second-order sections, applied forward only (not zero-phase), along the time axis of mono or stereo input; cutoffs clipped to 10 Hz .. 0.95 × Nyquist. |
| P4 | fractional feedback comb | z[i] = x[i] + g·z(i - D(i)), D(i) a fractional, time-varying delay in samples; z(i - D) is read by linear interpolation between the 2 neighbouring past outputs, only when i - D(i) ≥ 1 (else z[i] = x[i]). Section 4.6 gives the exact use. |
| P5 | zero-phase Hann smoothing | width s: k = max(3, int(s × 48000) rounded up to an odd number). w = symmetric Hann of k points (w[i] = 0.5 - 0.5 cos(2πi/(k - 1))), divided by its sum. Pad x with k//2 copies of its first and last values, convolve with w, keep the centred part of the original length. |
| P6 | control noise | n white samples, P3 low-pass order 2 at fc, divided by (standard deviation + 1e-12). A slow random curve with unit standard deviation. |
| P7 | colored noise by FFT | m = an FFT-friendly length ≥ n + 256. F = rfft of m white samples. Bin frequencies floored at 15 Hz. Divide F by f^0.5 (pink) or f^1 (brown). Inverse FFT, keep samples 128 .. 128 + n - 1, divide by (std + 1e-12). |
| P8 | mix level | the peak over 10 ms blocks (480 samples, whole blocks only) of the block RMS, + 1e-12. Whoosh variant: if there is no whole block, the RMS of all samples + 1e-12. Riser variant `lvl(x, frac)`: stereo is folded to the channel mean first; when frac is given, the measurement uses the final frac × len samples; at least 1 block is used. |
| P9 | fade | forge-core `fade(x, fin, fout)`: a = min(len, P1(fin)), b = min(len, P1(fout)). If a > 1, the first a samples times 0.5 - 0.5 cos(πi/(a - 1)). If b > 1, the last b samples times 0.5 + 0.5 cos(πi/(b - 1)). A side of 1 sample is not faded. |
| P10 | reverb | forge-core `reverb(x, r, rt60, size, predelay, damp, bass, mod, diffuse, early_db, width, wet, dry, late, lines, tail, stereo, air, lowcut, ...)`: a feedback-delay-network reverb with early reflections, built from the render generator. Defaults: rt60 1.6, size 0.6, predelay 0.01, damp 0.5, early_db -6, width 1, wet 0.3, dry 1.0, lines 16, lowcut 0. The output is longer than the input by the tail. |
| P11 | saturate | forge-core `saturate(x, drive_db, mode, mix, bias, os, level)`: tanh or tape soft clipping at drive_db, blended by mix, optional oversampling os, level matched by rms (default) or peak. |
| P12 | RBJ shelf | shelf(x, f0, g, high, S): A = 10^(g/40), w0 = 2π·min(f0, 0.45·SR)/SR, c = cos w0, α = (sin w0)/2·√((A + 1/A)(1/S - 1) + 2), q = 2√A·α. High shelf: b = [A((A+1) + (A-1)c + q), -2A((A-1) + (A+1)c), A((A+1) + (A-1)c - q)], a = [(A+1) - (A-1)c + q, 2((A-1) - (A+1)c), (A+1) - (A-1)c - q]. Low shelf: b = [A((A+1) - (A-1)c + q), 2A((A-1) - (A+1)c), A((A+1) - (A-1)c - q)], a = [(A+1) + (A-1)c + q, -2((A-1) + (A+1)c), (A+1) + (A-1)c - q]. Divide all by a[0]; apply as a direct-form IIR along the time axis. |
| P13 | forge-core noise | forge-core `noise(seconds, r, color)`: white, or pink / brown by FFT shaping (1/f^0.5 or 1/f, the DC bin given the value of bin 1), scaled to unit RMS. |
| P13b | darkriser pink | n white samples, FFT, divide each bin by √(max(f, 15 Hz)), inverse FFT of exactly n samples, unit standard deviation. |
| P14 | room, mic, trim | forge-core `room_ir` / `room(x, r, rt60, size, damp, wet)` (early reflections plus a noise tail, the wet part scaled by the IR energy), `mic(x, r, low_cut, air_db, floor_db, color)` (a microphone colour and noise floor), and `trim_tail(x, floor_db, keep)`: m = \|x\| for mono, or \|per-sample maximum over the channels\| for stereo (the maximum of the signed values, then the absolute value); keep everything up to the last sample with m ≥ max(m)·10^(floor_db/20), plus P1(keep) samples. |
| P15 | phase laid out from the end | for a per-sample rate r (Hz): c[i] = Σ_{j ≥ i} r[j]/SR (cycles from sample i to the end); phase = end_phase - c. So the pulse train is aligned to the last sample. |
| P16 | gate shape | gate_shape(phase, period, duty, sharp, ramp = 0.0012, perc = 0): fr = phase mod 1. trem = 0.5 + 0.5 cos(2π(fr - duty/2)). on = clip(fr·period/ramp, 0, 1), off = clip((duty - fr)·period/(1.5·ramp), 0, 1), square = min(on, off). If perc > 0: square = on·exp(-fr/max(perc, 0.001))·clip((0.98 - fr)·period/ramp, 0, 1). Output (1 - sharp)·trem + sharp·square. |
| P17 | 24 dB/octave filter | 2 cascaded P2 filters of the same mode (low or high) at the same cutoff, q 0.54 then q 1.31 (a 4th-order Butterworth pole pair: no peak). |
| P18 | decorrelate | forge-core `decorrelate(x, amount, cut = 250)`, mono in (a stereo input is folded to its channel mean first), stereo out, deterministic: side = x through 3 short all-passes (delays 149, 263 and 397 samples, gains 0.6, -0.55, 0.5), high-passed at cut (Butterworth order 2), RMS-matched to x and scaled by 0.7. L = x + amount·side, R = x - amount·side, both divided by √(1 + 0.49·amount²). |
| P19 | width | forge-core `width(x, w)`: mid/side, side scaled by w; a mono input is decorrelated first. |
| P20 | unison | unison(freq, wave, detune, width, phases): for each of the 5 offsets o in UNISON = (-1, -0.45, 0, 0.45, 1): f = freq·2^(o·detune/1200), v = P23 oscillator of f with start phase phases[k], θ = (clip(o·width, -1, 1) + 1)·π/4, L += v cos θ, R += v sin θ. Output × √2/5. |
| P21 | shepard tone | forge-core `shepard(seconds, r, up, rate, f_lo, f_hi, kind, q, tilt = -1.5)`: an endless glissando of octave-spaced voices under a raised-cosine bell over log frequency, unit RMS. |
| P22 | riser reverb | verb_ir(rt60, size 0.75, predelay 0.02, damp 0.5, lowcut 200): the forge-core reverb IR with 2 inputs, built from a generator with the fixed seed 1234, rt60 rounded to 0.1 s; cached per (rt60, size, predelay, damp, lowcut) rounded. wet_of(x, rt60, n_out, ...): x made stereo (mono duplicated); the wet output = Σ over input channels c of conv(x[:, c], IR[:, c, :]) (each input channel feeds both outputs); cut to n_out (default the full length). The return contains no dry signal. The noise of each render still varies; only the IR is fixed. |
| P23 | band-limited oscillator | forge-core `osc(freq, shape, seconds, phase)`: sine, PolyBLEP saw and square, triangle as a leaky integral of the square; freq may be an array; phase in cycles. |
| P24 | peaking EQ | forge-core `peq(x, f0, gain_db, q)`: an RBJ peaking biquad. |
| P25 | crush | forge-core `crush(x, bits, rate, mix)`: sample-and-hold to `rate` (index floor(floor(i·rate/SR)·SR/rate)), then quantise y to round(y·2^(bits-1))/2^(bits-1), blended by mix. |
| P26 | box window | box(n, edge = 0.0004): ones; k = min(n // 2, max(2, P1(edge))); the first k samples rise along 0.5 - 0.5 cos(πi/(k - 1)), the last k fall along the mirror image. |

Other forge-core functions the parts call by name: `modal` (a sum of damped sines, 14.5), `bell` with the BELLS
presets, `fm`, `air`, `granular`, `limit`, `place`, `sub_drop`, `reverb_ir` (part 5 notation).

## Constants

Unit-wide constants. Per-recipe constants (SPACE ranges, DEFAULTS, presets, edits, targets) are in the tables of
each part.

| constant | value | where |
|---|---|---|
| sample rate | 48 000 Hz | all |
| seed mask | seed & 0xFFFFFFFF | forge-core wrapper |
| silence threshold | absolute peak < 1e-7 → rejected | forge-core wrapper |
| delivered level | -1 dBFS peak | forge-core wrapper |
| internal peak | 0.89 | brightswish, darkriser, revcymbal, glitch v6, burstglitch, sting |
| UNISON offsets | -1, -0.45, 0, 0.45, 1 | riser, downlifter (P20) |
| fixed reverb IR seed | 1234 | P22 |
| semitone | 2^(1/12) | notify |
| C0 | 16.3516 Hz | shimmer scales |
| -50 dB decay factor | 5.76 (ln of 10^2.5) | hover gate |
| seed bank limits | per_context 6, cap 48, delivered entries pinned | forge-core seed bank |

## Data assets

### Seed banks

Format (forge-core part 7, "Seed bank"): `{"recipe": <module name>, "entries": [...]}`. Each entry has `params` (a
parameter set), `rank_score` (or null), `context` ("<target text> | <comma-joined reference file names>"), `total`,
`tone_db`, `passes`, `source`, and optionally `robust_mean`, `robust_pass_rate`, `delivered` (the category it was
delivered for), `was_delivered`, `member` (the member name) and `seed`. A delivered entry is pinned: saving never
drops it. The forge reads the bank to start searches near good sets; the bank (unit bank-kit-sfx) reads the delivered
entries.

| bank | entries | contexts (target text, entries) | delivered entries |
|---|---|---|---|
| whoosh.json | 27 | "a fast crisp air whoosh" 15; "a big deep whoosh transition" 6; "a deep low designed whoosh swell" 6 | 18, all version 2 (they have `rise`): whoosh_fast crisp, sleeve, whistle, heavy, reverse, tight; whoosh_big swell, cape, jet, deep, gust, reverse; pro_whoosh_big swell, reverse, long, windy, tonal, gritty. 6 more entries are marked was_delivered (earlier rounds). 3 undelivered entries are version-1 sets (no `rise`) |
| swish.json | 5 | "a thin quick swish of a cane through the air" 5 | swish cane, blade, stick, zing, soft |
| swipe.json | 5 | "a soft cloth swipe" 5 | swipe cloth, paper, velvet, flick, glide |
| riser.json | 22 | "a cinematic riser building tension into a hit" 11; "a short stuttering digital riser into a cut" 11 | riser riser_01 .. riser_05 (02, 03 and 05 are round-6 builds with r6 = 1; 01 and 04 are round 1 to 4 sets); pro_riser pro_riser_01 .. pro_riser_05 |
| downlifter.json | 6 | "a downlifter sound effect: a sinking filtered sweep" 6 | downlifter shepard_fall, flange_air, synth_fall, air_fall, tape, sub_sink |
| zip.json | 10 | "a quick tonal zip, a fast synth pitch sweep" 10 | pro_zip up_hi, up, arc, down, down_warm |
| glitch.json | 18 | "a short digital glitch stutter with bit-crushed noise bursts ..." 18 | pro_glitch pro_glitch_01 .. 06 (version 2); glitch stutter, crush, data, dropout, scramble, hit (version 6: they have `roll0`, seed 3) |
| notify.json | 23 | see 12.13 | pro_notify ding_1822 .. ding_2296 (round 5); notify notify_01 .. 06 (round-3 bells with the round-4 edit) |
| shimmer.json | 23 | see 13.9 | shimmer shimmer_01 .. 06; pro_shimmer pro_shimmer_01 .. 05 (04 and 05 need r2_path = 1) |
| hover.json | 17 | see 14.7 | pro_hover_tick ping, air, glass, soft, detent; hover_tick high, mid, ping, air, detent |
| sting.json | 28 | see 15.12 | pro_logo_sting: the 5 round-5 gestures; logo_sting: the 5 round-5 launch members |

Each delivered entry holds a full parameter set (the forge saved what it rendered), so a bank entry renders without
a merge. The undelivered entries are good sets from searches; the forge uses them as search starts. The member names
of the tuned sets (riser_01, notify_01, pro_glitch_01 and others) follow the delivery order, not the preset-table
order.

brightswish, burstglitch, darkriser and revcymbal have no seed bank.

Origin and class: **NEEDS-DATA** for exact reproduction. The source's entries came from forge searches whose critic
scored renders against licensed reference recordings, and their `params` were tuned by renders of the source's
exact random draws. A clean build can neither re-run those searches nor reproduce those renders.

Fallback: **DERIVE.** Build each bank from this spec's preset tables: one pinned entry per delivered member, with
`params` = the member's full parameter set (base + overrides + edits as the part states), `delivered` = the
category, `member` = the name, `seed` = 0, `rank_score` = null, `context` = "<the category's target> |" with no
reference names, `source` = "derived". Then let our own forge runs add entries (forge-core REGENERATE). Where the
delivered set was tuned by a search (whoosh, swish, swipe, riser, pro_riser, downlifter, pro_zip, pro_glitch, hover,
shimmer), the derived entry is the corresponding hand-designed member: the member of the same name where the bank uses
preset names, else the members in preset-table order. The tuned values themselves are NEEDS-DATA. An empty bank is also valid (forge-core Q4): searches then
start from DEFAULTS.

### Reference recordings

The critic's reference sets for these categories (forge splits of whoosh_fast, whoosh_big, pro_whoosh_big, swish,
swipe, riser, pro_riser, downlifter, pro_zip, glitch, pro_glitch, notify, pro_notify, shimmer, pro_shimmer,
hover_tick, pro_hover_tick, logo_sting, pro_logo_sting, reverse_swell) are licensed recordings and trims of
professional films. **NEEDS-DATA.** Fallback: designed mode (no references) with the study targets that the recipes
carry (`TARGETS` tables, `STUDY_STING`, `PRO_SHARES`, `measure` / `target_check` / `study_extra` / `pro_penalty`).
forge-core owns the registry (forge-core Data assets).

### Study numbers

The measured facts in the parts (pro film measurements, TARGETS and STUDY ranges) are kept as facts. **DERIVE**:
ship them as given. Re-measuring them needs the study films (NEEDS-DATA).

### Presets and tables

All preset tables, partial tables (PARTIALS, MATS, BAR, TUBE, TRIANGLE, G_MATS), chords and scales are numbers in
this spec. **REGENERATE**: type them in from the parts. No audio, model or binary asset belongs to this unit.

## Errors and edge cases

1. **Wrapper rejects.** A render that is empty, has a non-finite sample, or peaks under 1e-7 is rejected by the
   forge-core wrapper (None). Recipes do not raise for these; the forge skips the candidate.
2. **Missing keys.** Recipes that merge DEFAULTS accept partial sets. hover does not merge: a set without a key that
   it reads must raise a key error, not render with a guess. Its contact and grain keys are the exception: absent
   means off (14.7).
3. **Out-of-range values.** `render` does not clip to SPACE. Members outside SPACE exist and must render as given
   (notify interval -4, shimmer sizzle -3, hover contact 23.3). SPACE only bounds the search.
4. **Divisions.** Some keys are time constants or curve times that divide: notify att, hold_tau, ring_tau, body_tau;
   shimmer swell, fade; sting chirp_ms, strike_tau, ring_tau, g_tau1, g_tau2 and others. Their SPACE minimums are
   above 0. A value of 0 is outside the contract; a clean build may raise an error for it.
5. **The P1 minimum of 1 sample.** A delay of 0 s becomes 1 sample: whoosh low_lag 0 (part 1), hover's main contact,
   the first note of a sting chord or dyad, the first tap of taps_tone. These 1-sample offsets are inaudible. Keep
   them, or use 0 samples; either is acceptable (Q3).
6. **Short signals.** P8 needs at least one 10 ms block (the riser variant fails on a signal under 480 samples; the
   whoosh variant falls back to the full RMS). shimmer's bed reference needs a file of at least 0.2 s (SPACE minimum
   1 s). notify's measure returns attack 0 and no head_hz on a very short signal; t20 and t40 are null when the level
   never falls far enough.
7. **Reverb tails.** Reverb outputs are longer than their inputs. Each recipe states whether it pads its signal to the
   tail (notify chime and ding) or cuts the return to the dry length (most others).
8. **Non-finite reverb.** revcymbal and others keep the dry signal when the reverb returns a non-finite value (part
   2).
9. **Unknown modes and styles.** sting: an unknown mode renders the reveal idiom; an unknown gesture renders
   swell_sting. notify: an unknown style that is not bell or chime renders the ding path. shimmer: any style other
   than sparkle and run renders chimes.
10. **sting lead edge.** A launch lead of more than 0 but at most 64 samples is skipped without a zero prefix; a
    lead_level ≤ 0.005 means no lead.
11. **Source quirks kept in this spec.** The riser drone gets the level factor lv^0.3 twice (part 2, Q4). The notify
    tremolo phase is drawn from 0 to 1 radian, not 0 to 2π (12.10). The shimmer envelope ignores gate_end (13.5).
    trim_tail measures stereo by the per-sample maximum of signed values (P14). In the round-4 `bell` member of
    pro_logo_sting, 4 keys never took effect (15.5, Q2).

## Acceptance tests

"Render X" means: call the module's `render(p, numpy.random.default_rng(seed))` directly (not the forge wrapper),
with seed 1 unless a test says otherwise. Tolerances are stated; levels are of the raw recipe output.

| # | input | expected properties |
|---|---|---|
| AT1 | every recipe: render its DEFAULTS (hover: `family_params('hover_tick', 'high')`) | a float array of shape (n, 2), n ≥ 480, every sample finite, absolute peak ≥ 1e-4 |
| AT2 | every recipe: render the same p twice with seed 7, then once with seed 8 | the 2 seed-7 outputs are identical; the seed-8 output differs in at least one sample |
| AT3 | whoosh: DEFAULTS | n ≤ P1(4.5); the first sample is 0 (sin² fade-in); the last sample is 0 (the raised-cosine end fade); the loudest 10 ms block of the mono fold starts within 60 ms of the peak time tp of `envelope(p)` |
| AT4 | brightswish: DEFAULTS with comb_mix 0 | equal (to 1e-12) to the whoosh render of the same set without the 4 comb keys, with the same seed, then faded as 4.6 step 4 and scaled to peak 0.89 |
| AT5 | riser: DEFAULTS (rounds 1 to 4 path) | n = P1(DEFAULTS length) exactly; the loudest 10 ms block lies in the last 25 % of the file (the build ends on the hit) |
| AT6 | downlifter: DEFAULTS | n ≤ P1(length) + P1(min(1.5, 0.6·rt60)); the loudest 10 ms block lies in the first 40 % of the file |
| AT7 | zip: DEFAULTS | the mean of each channel is 0 within 1e-9 × the peak (the Hann DC removal, 7.7 step 13) |
| AT8 | darkriser: its first member | n = P1(length + tail_tau·60/8.686 + 0.02); absolute peak 0.89 within 1e-9 |
| AT9 | glitch: version-2 DEFAULTS (dur 0.65, end_tail 0.012) | n = P1(0.65) + P1(0.048) = 31200 + 2304 = 33504 samples; samples 0 and 1 are 0 in both channels |
| AT10 | burstglitch: each member | n = P1(dur) + P1(5·end_tail); absolute peak ≤ 0.89 + 1e-9 |
| AT11 | notify: FAMILY['pro_notify'][0] (ding_1822) | n = P1(0.26) = 12480; `measure` gives head_hz within 1772 to 1872 Hz and attack_ms ≤ 15; the energy under 500 Hz (low_db) is below -38 dB of the total; the first sample is 0 |
| AT12 | notify: FAMILY_R3['pro_notify'][0] (style success, interval 7) | the long-term spectrum has peaks within 2 % of 1822 Hz and of 1822 × 2^(7/12) = 2729.6 Hz |
| AT13 | notify: FAMILY['notify'][0] with R4_EDITS['notify'][0] applied | n = P1(2.3) = 110400; `pro_penalty(x, 'notify')` term for sub_db is 0 (sub_db ≤ -20); attack_ms ≤ 10 |
| AT14 | notify: any member with ring 1, soft 0, bell_len 0 added | identical output to the same member without those keys (same seed) |
| AT15 | notify: `ring_scaled({'style': 'ding', 'hold_tau': 0.1, 'body_tau': 0.2, 'decay': 0.5}, 2)` | hold_tau 0.2, body_tau 0.4, decay unchanged 0.5 (not a ding key) |
| AT16 | notify: `family_space(FAMILY['pro_notify'][0])` | f0 → [1822.0]; style → ['ding']; att → (0.0008, 0.012, 'log'); interval → [7] |
| AT17 | shimmer: `scale_freqs(400, 1000, 'penta', 0)`; `scale_freqs(500, 510, 'penta', 0)` | [440.00, 523.25, 587.33, 659.26, 783.99, 880.00] (to 0.01 Hz); [504.98] |
| AT18 | shimmer: `envelope` with start_db -42, swell 0.85, rise_curve 1.2, hold 0.1, fade 0.85, at t = 0, 0.85 and 1.8 s | 0.00794, 1.0 and 0.00562 (within 1 %) |
| AT19 | shimmer: FAMILY['pro_shimmer'][0] (sparkle) | n = P1(1.95) = 93600; `measure` corr < 0.6 (a wide sparkle) |
| AT20 | shimmer: FAMILY['shimmer'][0] (ring-out run) | n = P1(4.2) = 201600; the first sample is 0; the last sample is under 1e-9 × the peak |
| AT21 | hover: `family_params('pro_hover_tick', 'ping')`; `family_space('hover_tick', 'mid')` | ping 0.75, ping_f √(8000·14000) = 10583.0, length 0.035, grains 0.0, detent 0.0; tick_f → (3000, 5000, 'log'), detent → [0.0], snap → (1.0, 2.0) |
| AT22 | hover: DEFAULTS (a full set) | n = P1(0.002 + 0.045) = 2256 (ring = 5.76e-3·6 = 0.0346 s, so last + ring + release = 0.0446 < length); true peak ≤ -1 dBFS + 0.1 dB; the energy under 500 Hz is below -30 dB of the total |
| AT23 | hover: DEFAULTS with detent 0.5, detent_dt 0.012 | n = P1(0.002 + 0.012 + 0.0346 + 0.01) = 2811 |
| AT24 | sting: `{'mode': 'chirp'}` | n = P1(0.85) = 40800; L and R identical; absolute peak 0.89 within 1e-9 |
| AT25 | sting: FAMILIES_R4['pro_logo_sting'] member 'double' | n = 48000 + P1(1.25·0.04675) + P1(0.012) = 48000 + 2805 + 576 = 51381 |
| AT26 | sting: FAMILIES['logo_sting'] member 'whoosh_glass' | the sample of the absolute peak lies at or after P1(0.1896) - P1(0.012) = 8525 (the lead stays under the hit); peak 0.89 |
| AT27 | sting: every member of FAMILIES['pro_logo_sting'] | finite; peak 0.89; the energy above 20 kHz is below -40 dB of the total (the 16 kHz low-pass) |
| AT28 | sting: `space_for('pro_logo_sting')`; `recipe_for('logo_sting', 'spark')` and `recipe_for('logo_sting', 3)` | 15 keys, g_warm → (0.0, 0.45), g_lead → (0.0, 1.0); both recipe objects have PRESET 'spark', DEFAULTS material 'triangle', REFSET 'logo_sting' |
| AT29 | every seed bank: each delivered entry rendered through the forge-core wrapper with its seed | not None (finite, not silent); the result peaks at -1 dBFS |
| AT30 | the DERIVE fallback: build notify.json from the tables | 11 pinned entries; members ding_1822 .. ding_2296 under pro_notify and 6 notify entries; each `params` renders (AT1) |

## Dependencies on other units

| unit | what this unit uses or gives |
|---|---|
| core-io | nothing directly. The forge writes the WAV files and reads and writes the seed-bank JSON. |
| listen | nothing directly. The critic that scores these recipes uses the listen models through forge-core. |
| timeline | nothing directly. Timeline placement uses the delivered sounds (whoosh, riser and sting peaks, ends and apexes). |
| forge-core | the recipe contract and the render wrapper (part 1, part 7); the dsp and fx primitives P2, P3, P9 to P11, P13, P14, P18, P19, P21, P23 to P25 and modal, bell and BELLS, fm, air, granular, limit, place, sub_drop, reverb_ir; the seed-bank load and save; families, palettes and `family_box`; the knobs that set length, pitch, envelope shape, decay, onset, brightness and width keys of these recipes; the critic's extra hooks. |
| recipes-impacts | the impact recipe: its comb core (brightswish), `hall_ir`, `measure`, `study_extra`, `study_penalty`, its final `impact` family member 'punch' and `render` (sting reveal hit); the subdrop recipe: `render` and the members `sub_drop` '808' and `pro_sub_drop` 'ref' (sting); the click recipe helpers partials, burst, gate, narrow and centre (hover, restated in 14.5). In the other direction, its thump recipe renders on the whoosh pass engine (whoosh `render` and `OPTIONAL`), and its pop recipe uses the same pro-study measurements as notify `measure`. |
| recipes-motion | this unit. Inside it: swish, swipe and brightswish use the whoosh engine; downlifter and zip import the riser helpers (5.10); shimmer imports notify `measure` and the outside distance. |
| bank-kit-sfx | the bank and the kit read the delivered members (seed banks, FAMILIES via the palette, `preset_full`) and use these categories (riser, glitch, whoosh, sting, shimmer and others) by name; gated and designed category lists name glitch, riser and zip categories. |
| fit | nothing directly. |
| surfaces | the CLI and MCP tools expose the forge and the knobs that run these recipes. |
| skills-and-docs | the skill text names these sound families and how to ask for them. |

## Open questions

| # | question | default in this spec |
|---|---|---|
| Q1 | The sting reveal hit uses member index 2 of the impact recipe's final `impact` family. In the source that is 'punch' (the final list is classic, deep, punch, metal, swell, blast). The recipes-impacts spec must keep that order, or the sting must name the member. | name it: use the member 'punch' |
| Q2 | In the round-4 `bell` member of pro_logo_sting, a source comment hides 4 keys (chirp_f0 1900, chirp_f1 190, chirp_ms 75, hum_f 84). Keep the effective values (1500, 150, 85, 76) or the intended ones? The family is kept, not delivered. | effective values |
| Q3 | whoosh low_lag 0 (and other 0 s offsets) delays by 1 sample through P1. Keep the 1 sample or use 0? | either is acceptable |
| Q4 | The riser drone gets lv^0.3 twice. Keep it (it is in every delivered riser) or fix it? | keep |
| Q5 | Seed banks: ship the DERIVE banks of this spec, or empty banks (forge-core Q4)? | derived banks; empty is valid |
| Q6 | Old render paths (whoosh v1, riser rounds 1 to 4, glitch v2, shimmer r2_path, sting chirp and reveal, notify round-3 dings) serve older and undelivered sets. Keep all of them? | keep all: the seed banks hold such sets |
