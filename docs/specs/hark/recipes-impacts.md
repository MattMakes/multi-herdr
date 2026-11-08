# recipes-impacts: the impact and UI recipes (index)

This is the behaviour spec of the hark unit `recipes-impacts`. It covers 16 recipes, their seed files and the pluck
bank file. A clean implementer can rebuild the unit from this index and its parts, with no access to the source.

The spec is longer than 60 KB, so it is split. This index holds the shared contract, the primitives, the tests, the
dependencies and the open questions. The parts hold the recipes.

## Parts

| Part | File | Contents |
| --- | --- | --- |
| UI | `recipes-impacts-ui.md` | `click` (with the shared UI layer functions L1-L9), `toggle`, `ui_pop`, `shutter` |
| keys | `recipes-impacts-keys.md` | `keypress`, `typing` (one key and the typed run) |
| tonal | `recipes-impacts-tonal.md` | the shared tonal building blocks T1-T15 (envelopes, tones, bursts, strikes, finish, catalog measurement, target distance) |
| pop | `recipes-impacts-pop.md` | `pop` (version 1 and version 2 with all palettes), `popin` |
| impact | `recipes-impacts-impact.md` | `impact` (version 2, the stand-alone riser `swell`, version 3 with families, search spaces, pro-study measurement and study penalty) |
| hits | `recipes-impacts-hits.md` | `cuthit`, `subdrop`, `thump`, `knock` |
| tuned | `recipes-impacts-tuned.md` | `mallet` (with the run builder), `drip` (with the audit allowance), `pluck` |
| seeds | `recipes-impacts-seeds.md` | the 8 seed files, `bank/pluck.json`, the reference clips, with REGENERATE / DERIVE / NEEDS-DATA marks |

Each recipe section in a part has the same shape: sound (with the measured film targets), interface fields,
parameters (range, scale, default, meaning), families or palettes, building blocks, render algorithm, variations.

## 1. Interface

### 1.1 The recipe module

Each recipe is one module of the hark recipes package, named after the recipe. The forge finds a recipe by its module
name. A module exposes these public fields. The parts say which fields each recipe has.

| Field | Type | Meaning |
| --- | --- | --- |
| `TARGET` | string | the text the critic's audio-text embedding aims at |
| `TARGETS` | map category -> string | the target text per forge category |
| `ALTS` | list of strings | contrast texts the critic must not match |
| `REFSET` | string | the default reference-set category of the critic |
| `REFS` | list of paths | reference clips under the reference root (1.4) |
| `SPACE` | map key -> range | the parameter space (1.2) |
| `DEFAULTS` | map key -> value | the default parameters |
| `FAMILIES` | map category -> list of (name, params) | named members; params may be partial (merged over a base) |
| `FAMILY` and `FAMILY_NAMES` | map category -> list of full dicts, and map category -> list of names | the tonal recipes' palette form (knock, mallet, drip, ui_pop, shutter) |
| `FINE` | list of keys | the keys a family search may move (tonal recipes) |
| `STUDY` (and named variants) | map | measured study targets of the recipe |
| `render(p, rng)` | function | the sound (1.3) |

Helper functions, where a part defines them: `family_space`, `family_params`, `space_for(cat)`, `recipe_for(cat,
preset)`, `preset`, `preset_full`, `recipe`, `run_freqs`, `render_run`, `note`, `allows`, `check`, `measure`,
`study_penalty`, `study_extra`.

A second recipe object is a plain record with the fields module name, SPACE, DEFAULTS, render, TARGET, ALTS (and
REFSET where given). The forge treats it like a module. There are 3: the pop version 2 object (part pop), the typed
run object (part keys), the chime run object (part tuned). A recipe object made by `recipe_for` or `recipe` also has
PRESET (the member name).

### 1.2 Space notation

| Form | Meaning |
| --- | --- |
| `(lo, hi)` | a linear range of floats |
| `(lo, hi, 'log')` | a log range: a search moves the value on a log scale; lo > 0 |
| `(lo, hi, 'int')` | an integer range, both ends included |
| a list | a set of choices (strings or numbers); a one-choice list pins the value |

A family space is a space whose ranges are narrowed or pinned around one member (L9 in part UI, T15 in part tonal).

### 1.3 Render and output

`render(p, rng)`:
- p is a parameter dict. Most recipes merge DEFAULTS under p first, so p may be partial. Exceptions are named in the
  parts (pluck does not merge).
- rng is the random generator of the take. Every random draw of the render comes from it, or from a child generator
  seeded by a draw from it, or from a fixed per-key generator the part defines (keypress, typing).
- The result is a float64 array at 48 000 Hz: mono shape (n,) or stereo shape (n, 2).
- Several recipes normalise to a peak of 0.89. Others leave the level to the caller. The parts say which.

Output: a recipe may return mono. The forge-core consumers (the critic, the kit writer, the audition writer) copy a
mono render to 2 channels. The forge search renders a candidate, then peak-normalises it to -1 dBFS.

### 1.4 Reference root

The reference clips live under a root folder. The source reads it from an environment variable with the default
`refs`. Hark uses the variable `HARK_REFS`, default `refs`. Backslashes become `/` and a trailing `/` is removed. A
recipe names its clips as `<root>/<sub-folder>/<file>`. The clips are not in the repo (part seeds, S4).

### 1.5 Determinism

- A take generator is a numpy-compatible PCG64 generator seeded with seed & 0xFFFFFFFF.
- The same parameters and the same seed give the same output in hark, on every run.
- Different seeds give different takes where the recipe draws noise or phases.
- Bit-exact equality with the source is not a goal. The draw orders in the parts keep the take structure (which
  layers share a take) and are a guide, not a contract with the source's numbers.

## 2. Behaviour

| Recipe | Part | Sound | Categories | Channels |
| --- | --- | --- | --- | --- |
| `click` | UI | a UI click and its pro tick | `click`, `pro_click` | stereo |
| `toggle` | UI | a switch flipped with 2 quick clicks | `toggle` | stereo |
| `ui_pop` | UI | a tonal UI pop (film catalog palette) | `ui_pop` | stereo |
| `shutter` | UI | a camera shutter | `shutter` | stereo |
| `keypress` | keys | a mechanical keyboard key, built from physics | `typing_key` | stereo |
| `typing` | keys | a high-passed digital typing tick, and a typed run | `pro_typing_key`, `pro_typing_run` | stereo |
| `pop` | pop | a bubble pop (v1) and designed pops (v2: chirp, blip, bubble) | `pop`, `pro_pop` | mono or stereo |
| `popin` | pop | a soft round pop-in, a finger tap | `pro_pop_in`, `pro_tap`, `tap` | stereo |
| `impact` | impact | a cinematic boom and a designed sub impact | `impact`, `pro_impact` | stereo |
| `cuthit` | hits | a short sub hit with a kick, on a cut | `cut_hit` | stereo |
| `subdrop` | hits | a gliding sub drop | `sub_drop`, `pro_sub_drop` | stereo |
| `thump` | hits | a low whoosh that lands with a thump | `whoosh_big` | stereo |
| `knock` | hits | a wood or plastic knock | `knock` | stereo |
| `mallet` | tuned | gated mallet notes, a soft pluck, chime runs | `mallet`, `chime_run` | stereo |
| `drip` | tuned | a water drop | `drip` | stereo |
| `pluck` | tuned | a kalimba tine | `pluck` (bank) | stereo |

Shared behaviour:
- No recipe uses recorded audio. Every sample is computed.
- Each recipe is tuned against measured film targets (length, attack, decay, centroid, band shares, tonal share,
  width). The parts give the targets. A hark render of a member must land in the same ranges (section 7).
- A family member is a corner of the character keys. A forge search may move only the fine keys, so a search cannot
  move a member off its character.
- Every file ends on a fade and never on an audible step.

## 3. Algorithms

The algorithms are in the parts, as numbered steps. Shared notation:
- t = k / 48000 is the time of sample k in seconds.
- N(s) = max(1, round(48000 s)) is the sample count of s seconds.
- g is one standard normal draw from the take generator. uniform(a, b) is one uniform draw.
- "Normalise" divides by (peak + 1e-12). The small constant guards against silence.
- "Butterworth order k" is a second-order-sections Butterworth filter (P-BUTTER).
- A "raised-cosine" fade multiplies by 0.5 - 0.5 cos of a ramp 0 -> pi (in) or its mirror (out).

The code families of the parts: L1-L9 (UI layers), T1-T15 (tonal blocks), K1-K14 (keypress), Y1-Y3 (typing), V1-V5
and W1-W8 (pop), I1-I9 and J1-J14 (impact), C1-C8 (cuthit), M1-M7 (mallet), B1 (drip), P1-P8 (pluck). They name
building blocks inside this spec only. The clean side picks its own names.

## 4. Constants

| Constant | Value |
| --- | --- |
| sample rate | 48 000 Hz |
| generator seed mask | 0xFFFFFFFF |
| common output peak | 0.89 (cuthit, subdrop, thump, and others named in the parts) |
| forge candidate peak | -1 dBFS |
| reference root default | `refs` (hark variable `HARK_REFS`) |
| "off" level of a dB layer key | -59 dB or less in the tonal recipes (the space minimum is -60) |

Every recipe constant (mode ratios, family tables, study targets, filter corners) is in its part.

## 5. Primitives (forge-core)

The recipes call these forge-core primitives. The forge-core spec defines them in full. This table is the contract
the recipes rely on.

| Code | Contract |
| --- | --- |
| P-N | sample count: max(1, round(48000 s)); time axis k / 48000 |
| P-NOISE | noise of s seconds, colour white, pink (1/f power) or brown (1/f^2 power), scaled to RMS 1; the colours are shaped in the frequency domain from 1 normal vector |
| P-BUTTER | Butterworth filter, kinds low, high, band (lo, hi), band-stop; second-order sections; corners clipped to 10 Hz .. 0.475 x 48000; filters along the time axis, so stereo works |
| P-PEQ | peaking EQ biquad (centre, gain dB, q; default q 0.9), centre clipped to 0.475 x 48000 |
| P-SVF | time-varying state-variable filter (topology-preserving transform); cutoff and q may be per-sample arrays; cutoff clipped to 20 Hz .. 0.45 x 48000; modes low, band, high, bandn. Low and high have unity pass band; band peaks at q at the cutoff (q 8 = +18 dB); bandn is band normalised to peak 1 |
| P-MODAL | a sum of damped sines (frequencies, time constants, amplitudes), per-mode detune jitter and an optional downward glide; modes at or above 0.475 x 48000 or with amplitude 0 are skipped |
| P-CONTACT | a strike excitation of s seconds (at least 2 samples): a raised-cosine force pulse plus pulse-shaped noise scaled by brightness |
| P-EXCITE | a resonator response driven by an excitation: their convolution, cut to the response length |
| P-ROOM | a small procedural room: an impulse response of an exponential noise tail (-60 dB at rt60) that darkens over time by damp, plus 6 early reflections whose spread grows with size; the dry signal stays at 1, the wet is added at `wet` relative to the response energy; output = input length plus the tail; a stereo input gets the same room on both channels |
| P-MIC | microphone: first-order high-pass at low_cut, a peaking EQ of air_db at 10 kHz (q 0.6), an optional presence colour, and a pink noise floor at floor_db relative to the peak, independent per channel |
| P-STEREO | mono to stereo: side = the signal delayed by d1 minus the signal delayed by d2 (d1, d2 uniform 0.2-0.9 ms), high-passed at 300 Hz; L = x + width side, R = x - width side; constant-power pan |
| P-DECOR | mono to stereo without random draws: side = the signal through 3 short prime all-passes, high-passed at `cut` (the low end stays mono); L = x + amount side, R = x - amount side |
| P-FADE | raised-cosine fade in over `fin` s and fade out over `fout` s (a fade shorter than 2 samples is skipped) |
| P-TRIM | cut trailing samples below floor_db relative to the peak (channel maximum), keeping `keep` s after the last loud sample |
| P-LIMIT | look-ahead peak limiter to a ceiling, linked stereo, optional true-peak measure, final safety clip |
| P-SAT | oversampled saturation (tanh, tape, hard, fold), drive dB, dry/wet mix, bias, level matching by RMS or peak |
| P-EARLY | early reflections only: a sparse tapped delay per channel, stereo out |
| P-VERB | a smooth algorithmic reverb (a delay network onset and a stochastic late field) with rt60, size, predelay, damp, wet, dry (default 1), width and stereo output |

## 6. Data assets

Part seeds gives the format, the meaning and the mark of each asset. Summary:

| Asset | Mark |
| --- | --- |
| Ordinary seed entries of the 8 seed files | REGENERATE |
| Delivered palettes that match a family table in a part | DERIVE |
| Delivered `pop` palette (pop_01 .. pop_05) | NEEDS-DATA (fallback: 5 seeds of the v2 bubble defaults, or a hark forge run) |
| Delivered `typing_key` palette (keypress) | NEEDS-DATA (fallback: 6 `key` values of the defaults, or a hark forge run) |
| Delivered `pro_typing_key` and `pro_typing_run` palettes | NEEDS-DATA (fallback: 6 seeds of the defaults each, or a hark forge run) |
| Critic scores in the seed files | REGENERATE |
| `bank/pluck.json` content | DERIVE (members are formulas over the pluck defaults) |
| `bank/pluck.json` measurements | REGENERATE |
| Reference clips (click, impact, keypress, pop, pluck, and the film and kit clips in the seed contexts) | NEEDS-DATA (fallback: run the critic without references) |

## 7. Errors and edge cases

- An unknown family member name in `recipe_for`, `preset` or `recipe` is an error. An index out of range is an error.
- An unknown choice value (a material, a bar, a scale, a style) is an error.
- `pluck` does not merge its defaults. A missing space key (other than knock_lvl, zing_lvl, rumble) is an error.
  `f0` absent or 0 picks a reference pitch.
- The forge search treats a render that raises, returns no samples, returns a non-finite value, or peaks below 1e-7
  as a failed candidate. It reports the first failure of each recipe and skips later ones.
- Very short buffers: N(s) is at least 1 sample. A layer that does not fit is skipped (thump: 16 samples or fewer
  after the strike; knock bounce: less than 8 samples left; mallet run: notes cut at the buffer end).
- Length caps: knock and drip at 1 s before the finish, a mallet note at 2 s, pluck at 3 s, subdrop at the -70 dB
  point of its decay (at least 0.3 s).
- High pitches: the mallet note skips partials at or above 0.42 x 48000, the pluck harmonics stop at 0.4 x 48000, the
  knock modes stop at 0.42 x 48000, T5 fades tonal partials over the last 2400 Hz below 21 600 Hz. The pluck's 2nd
  bending mode has no guard (open question Q-PLUCK-2).
- Silence and division: every normalisation adds 1e-12 to the peak. Time constants have floors (1e-4 or 1e-5 s).
- A mono render is valid (pop v2 chirp and blip, pop v1). Consumers copy it to 2 channels.
- Some family members hold values outside the full space (open question Q-RANGE-1). A render accepts them. A search
  must not clip them silently.
- `drip.allows` returns true for an empty glide result or one without a start time.

## 8. Acceptance tests

Tests with exact numbers use hark renders only. "Median of 8 takes" means seeds 1-8.

1. Contract. For each of the 16 recipes, render DEFAULTS (pluck: DEFAULTS plus f0 = 523.25) with seed 1. The result is
   float64, shape (n,) or (n, 2), finite, with a peak above 1e-7. The recipes that normalise peak at their stated
   level.
2. Determinism. For each recipe, 2 renders with the same parameters and seed are equal sample for sample. Renders with
   seeds 1 and 2 differ.
3. Peak. cuthit, subdrop and thump renders of every family member peak at 0.89 within 1e-9.
4. subdrop length. Member `pro`: seconds = max(0.3, min(1.4, 0.38 + 0.05 + 70 / 42)) = 1.4, so n = 67 200 samples.
   Member `punch`: life = 0.004 + 0 + 70 / 70 = 1.004 s, length 0.8, so n = 38 400.
5. subdrop band. With click = 0, thump = 0, grit = 0, room = 0, width = 0, the energy above 400 Hz is at least 60 dB
   below the total energy (the harmonics stop below 300 Hz; the floor is at -80 dB).
6. subdrop search space. `space_for('pro_sub_drop')` has exactly the keys harm, harm_lp, click, click_f, click_ms,
   length, with length (0.9, 1.6) and click_ms (15, 45, log). `space_for('sub_drop')` has 5 keys with the full ranges.
7. thump. With thump_db = -40, a thump render equals the pass engine render of the same parameters and seed, scaled
   to peak 0.89 (the thump is drawn but not added).
8. knock profile. Member `wood`: on the median of 8 takes, at least 5 of the 7 study keys (T14 measurement) are inside
   their study range.
9. knock family space. `family_space(wood)` pins mat, f1, bend, body and jit to one-choice lists of the member's values
   and keeps the full ranges of the 24 fine keys.
10. mallet gate. Member `glock` with jit = 0 and room = 0: M7 = min(2, 0.08478 + 7 x 0.0422 + 0.02, 0.00162 + 7 x 0.5027 + 0.02) =
    0.39018 s. The level is 40 dB below its peak before 0.5 s.
11. run frequencies. `run_freqs` with f0 1046.5, scale penta, notes 4, span 2, pattern up gives semitones 0, 4, 9, 14.
    With scale lydian, notes 3, span 2, pattern arc it gives semitones 0, 4, 0.
12. note names. `note('C5')` = 523.2511 Hz, `note('A4')` = 440, `note('F#4')` = 369.994, `note('Bb3')` = 233.082
    (4 decimals). `note('H4')` is an error.
13. drip allowance. `allows` is true for {}, for a glide without from_s, and for (rise 5 st, from 0.01 s, to 0.15 s).
    It is false for (rise 15, from 0.01, to 0.15), (rise 5, from 0.08, to 0.2) and (rise 5, from 0.0, to 0.3).
14. drip profile. Member `drop`: on the median of 8 takes, at least 4 of the 7 study keys (T14 measurement) are
    inside their study range.
15. pluck pitch. With f0 absent, the strongest spectral peak in 300-1000 Hz of each of 8 takes is within 1 % of one of
    392, 440, 493.9, 587.3, 659.3 Hz. With f0 = note('C5'), it is within 1 % of 523.25 Hz.
16. pluck bank. The 3 bank members' params equal the pluck defaults with the edits of part seeds S3.3 (kalimba_soft
    p2_lvl = 0.2895, knock_lvl = 0.28215; kalimba_ring tau1 = 0.52935), f0 = 523.2511306011972, seeds 8001, 8009,
    8006.
17. Seed store. A forge run of a recipe with no seed file starts from DEFAULTS. After a delivery, the seed file has the
    S1.2 fields, the delivered entries first, and a reload in the same context returns the delivered params first.
18. Mono output. A pop v2 chirp with no room returns shape (n,). The critic and the kit writer accept it and treat it
    as 2 equal channels.

## 9. Dependencies on other units

| Unit | Relation |
| --- | --- |
| core-io | reads and writes audio files (references, kits, auditions) and JSON; no direct call from a render |
| listen | the critic's analysis models (the audio-text embedding that scores TARGET and ALTS, and the reference comparison) |
| timeline | none; recipes do not place events on a timeline |
| forge-core | the primitives P-* (5), the generator, the recipe registry and the reference root, the search and the seed store load and save (part seeds S1.4), the critic, the audit (the rising-glide check that drip's allowance relaxes), the comb-ring detector named in subdrop's constraints |
| recipes-impacts | this unit |
| recipes-motion | the whoosh pass engine and its big-whoosh preset (thump), the notify measurement (pop version 2 targets), the click layers L1-L9 that the motion recipe hover reuses |
| bank-kit-sfx | the bank file format and its measurements (`bank/pluck.json`), the kits that pick delivered members, the sound check that applies drip's allowance |
| fit | none; the fit resynthesis does not call these recipes |
| surfaces | the forge and bank commands expose the recipes and categories by name |
| skills-and-docs | the motion and foley study documents that the parts' measured targets come from (summarised in the parts) |

## 10. Open questions

| Code | Question | Default if unanswered |
| --- | --- | --- |
| Q-POP-1 | The `pop` category members have only numbered names in the source (pop_01 .. pop_05, in the seed file). Name them? | keep numbered names |
| Q-POP-2 | The pop v2 natural chirp: after the 2.5 ms Hann window of the pre-tick, the un-windowed decayed sine resumes, a step of about 0.53 at that sample. Replicate or smooth it? | smooth it (a step is an audible click) |
| Q-EXACT-1 | Bit-exact equality with the source is impossible without its code. The tests check measured distributions and exact formulas instead. Accept? | yes |
| Q-RANGE-1 | Some family values leave the full space: click `thock` body_f and hp, popin `tap` knuckle att_ms and the tap hp values. Widen the space, or clamp the members? | widen the space to include them |
| Q-TONAL-1 | The shared tonal helpers are not in the ticket's file list, but 5 recipes need them. They are specified in part tonal. Does this unit own them? | this unit owns them |
| Q-PLUCK-1 | `pluck` does not merge its defaults, so a partial parameter dict fails. Merge in hark? | merge (a partial dict then works; full dicts behave the same) |
| Q-PLUCK-2 | The pluck's 2nd bending mode has no Nyquist guard (f0 above about 3.4 kHz with p2_ratio 7 aliases). Add a guard? | skip the mode at or above 0.45 x 48000, as for the 3rd and 4th |
| Q-SEED-1 | The keypress, typing and pop palettes exist only in the seed files (NEEDS-DATA). Use the fallbacks, or obtain the values? | use the fallbacks and run hark forge deliveries |
| Q-REFS-1 | The reference clips are not in the repo (NEEDS-DATA). Which clips does hark use? | run the critic without references until hark has its own set |
