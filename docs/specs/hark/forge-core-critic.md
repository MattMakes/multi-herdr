# forge-core part 8: the critic

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.critic`: the scorer that judges made
sounds against a text target, text confusions and real reference recordings. It combines model terms (from the
listen unit) with the model-free terms of parts 3 and 4.

One text-audio (CLAP) embedding per sound feeds every model judgement (the vocabulary calibration and the realness
head use the same padding), so a batch of 16 sounds costs about one batch of embedding windows.

## Interface

Module constants:

| name | value |
|---|---|
| `FEATS` | `attack_ms, decay_ms, brightness_hz, length_s, low_end_db, bandwidth_hz` |
| `LOG` | the FEATS compared in log units: `attack_ms, decay_ms, brightness_hz, length_s, bandwidth_hz` |
| `W2` | `{'match': 1.0, 'real': 2.0, 'typ': 0.5, 'feat': 0.7, 'alt': 1.0, 'tone': 1.0, 'env': 1.0, 'art': 1.0, 'polish': 0.75}` |
| `DESIGNED_WORDS` | glitch, stutter, sub drop, sub-drop, subdrop, blip, beep, bleep, synth, riser, downlifter, uplifter, stinger, logo sting, braam, laser, zap, notification, shimmer, bitcrush, digital |
| `ENV_PASS`, `POLISH_PASS`, `TONE_PASS` | 1.5, 1.5, 1.25 |
| `GATED` | glitch, pro_glitch, riser, pro_riser, zip, pro_zip, pro_impact (the stop is the design) |
| `STUDY_TARGETS` | absolute fallback targets per category (Constants) |
| `SHARE_KEYS` | `sh150: '<150', sh1k: '150-1k', sh4k: '1-4k', sh12k: '4-12k', shhi: '>12k'` |

Functions: `study_measures(x)`, `widen_targets(targets, ref_measures)`, `study_penalties(sm, targets)`,
`holdout_key(mono)`, `resolve_mode(mode, target, refs)`, `total_v2(r, W=None, mode='real')`.

Class `Critic(target, alts=(), refs=None, k=3, extra=None, tone=0.0, tone_target=None, mode='auto', version=2,
weights=None, category=None, study=None, gated=None)`.

Attributes: `target`, `alts`, `extra`, `tone`, `tone_target`, `version`, `W` (weights), `refs` (list of audio file
paths), `mode` (`real` or `designed`), `category`, `study` (target dict), `gated`, `k`, `tone_ref` (ToneRef or None),
`env_ref` (EnvRef or None), `polish_ref` (PolishRef), plus the embeddings of the text queries and the refs.

Methods: `embed(xs, batch=16)`, `score(xs, E=None, exclude=None)`, `score_files(paths)`, `score_refs(loo=True)`,
`consistency(xs)`, `holdout(xs, cache=True)`.

Services from the listen unit (by role): the vocabulary (`embed_text`, `calibrate(E, Q)` giving a z per query and
sound against the vocabulary, `top_terms(E, k)`, `embed_files(paths)`, the padding mode), the realness model
(`load_model`, `audible`, `signal_features`, `head_logit`, the signal vector, `fuse`, `pure_tone`, the list of synth
classes of the tagger), the feature measurement (`measure(x, light=True)`), the comparison features
(`feats(measure, x)` giving the FEATS keys) and typicality (`_typicality(e, A, k)` giving a z of closeness to the
nearest refs against the refs' own closeness), the audio tagger (AudioSet probabilities, top labels, label index),
the text-audio embedding of audio (`embed_audio(list, pad, batch)`), and the model cache naming.

## Behaviour

### Construction

1. Weights W = W2 overlaid by `weights`. When tone > 0 and `weights` has no `tone` key, W['tone'] = tone (callers of
   the first version keep their meaning).
2. Embed the target and the alternatives as text queries (target first).
3. Load the realness model.
4. refs: the audio files in the given files or folders, else [].
5. mode = resolve_mode(mode, target, refs): `real` and `designed` pass through; `auto` gives `designed` when there are
   no refs or the lower-case target contains any DESIGNED_WORDS entry, else `real`.
6. study = a copy of STUDY_TARGETS[category] (empty when unknown) unless `study` is given (then a copy of it; `{}`
   switches the fallback off). gated = category in GATED unless given.
7. With refs: embed the ref files; load each; measure the comparison features of each; compute the feature stats;
   build ToneRef and EnvRef (names = the paths).
8. polish_ref = PolishRef(ref arrays, designed = mode is designed, gated = gated and designed).

Feature stats: for each FEATS key with at least 2 ref values (log keys in ln max(v, 1e-3)): (median, max(IQR / 1.35,
floor)) with floor 0.15 for log keys and 1.5 for linear keys.

### `embed(xs, batch=16)`

The text-audio embedding of the mono mix (float32) of each sound, with the vocabulary's padding mode.

### `score(xs, E=None, exclude=None)`

One row per sound. `exclude[i]` is the index of a ref to leave out of sound i's comparison (leave-one-out scoring of
the refs), or None.

1. E = embed(xs) unless given. Calibrate E against the queries: z per sound and query.
2. For each sound:
   a. Ref set: all refs and the full feature stats; with an exclusion, the refs without that one and stats rebuilt
      from the others (memoised per excluded index). Typicality needs at least 3 refs in the set.
   b. When the mono mix is empty or not audible: row `{'i', 'total': -inf, 'passes': false, 'summary': 'silent'}`.
   c. match_z = z of the target. alt = the max z of the alternatives (-inf with none). margin = match_z - alt, or 9.0
      when there is no alternative.
   d. Realness p: the signal features of the mono mix; the CLAP head logit; the signal head logit (0.0 when there are
      no signal features); fuse them into a probability. When the signal features say pure tone, p = min(p, 0.03).
   e. Feature penalty: the comparison features of the stereo version (mono duplicated). For each stats key the sound
      has: z = (value - median) / sd (log units for LOG keys); record dev (2 decimals); add max(0, |z| - 1.5).
      feat_pen = sum / max(1, count).
   f. Typicality (when the ref set has >= 3 refs): typicality z from the listen unit with k neighbours; nearest_ref =
      the basename of the ref with the highest embedding dot product.
   g. The first-version total: min(match_z, 6) + 2 (2p - 1) + 0.5 clip(typ, -3, 2) (0 when typ is None) - 0.7
      feat_pen - max(0, 0.5 - margin). First-version pass: match_z >= 3 and margin > 0 and p >= 0.77 and (typ None or
      typ >= -2) and feat_pen <= 1.
   h. Row keys: `match_z`, `alt_z` (or None), `alt_zs` (each alternative's z), `alt_margin`, `p_real` (3 decimals),
      `ref_typ_z`, `nearest_ref`, `feat_pen`, `feat_dev`, `feats`, `total`, `passes`, `sounds_like` (the top 3
      vocabulary terms), `mode`; `left_out` (the excluded ref's basename) with an exclusion.
   i. Tone (when there is a ToneRef with refs, and its check is not empty): with tone_target, tone_ratio = tone_db /
      tone_target (2 decimals). Row `tone` = {tone_db, tone_ratio, outside, t40_ms, t40_ref, length_s, length_ref,
      shares, shares_ref_med, words}; `tone_db`, `tone_ratio`; `tone_pen` = ToneRef.penalty. far = the max of the
      leave-one-out tone distances (None when empty); with tone_target, far = 1.6 tone_target (the median-to-farthest
      ratio of a typical real set is about 1 : 1.6). In version 1 with tone > 0: total -= tone x tone_pen, and the
      pass fails when tone_db > far. `tone_far` = far exists and tone_db > TONE_PASS x far.
   j. Model-free terms (below).
   k. `total_v1` = the total so far. In version >= 2: total and passes = total_v2(row, W, mode).
   l. extra: when an `extra(x, row)` function is given, add its value to the total; record `extra`.
   m. `summary` (below).

Model-free terms:
- G = bandgram of the sound (computed once).
- EnvRef (when it has refs and its check is not empty): row `env` = {env_db, env_ratio, env_ref, env_out,
  env_out_ref, nearest, anchor, stretch, shift_ms, outside, words}; `env_db`, `env_ratio`, `env_out`; `env_pen` =
  EnvRef.penalty.
- PolishRef check (with the exclusion and G): row `artifacts` = {pen, hard, limits of the penalised keys}; `polish` =
  {pen, limits of the penalised keys}; `measures` (the measure dict without `res_peaks`); `art_pen`, `polish_pen`,
  `hard`, `polish_words`.
- Study fallback (below).

### Study fallback (thin reference sets)

Tone and envelope penalties are relative to the refs' own spread, which needs 3 refs. With 0-2 refs (many designed
categories) they were 0 and polish had no range, so the search was steered only by the models and the artifact
rules. The study fallback adds absolute targets for those categories, taken from a study of professional films and
widened by about a third because 1 or 2 films were measured per row.

Applies only when `study` is not empty and the ref count (minus the exclusion) is under 3.
1. The refs' study measures are computed once per critic. tg = widen_targets(study, the measures of the refs other
   than the excluded one): each range grows to include every ref's value (a trait the refs share is not a defect).
2. sm = study_measures(sound). pen, off, words = study_penalties(sm, tg).
3. When the row has no tone_ratio and pen.tone > 0: add it to tone_pen. When the row has no env_ratio and pen.env > 0:
   add it to env_pen. When pen.polish > 0: add it to polish_pen. (These run before total_v2, so they count.)
4. Row `study` = {measures, targets, off, pen (the parts used), words}.

`study_measures(x)` (mono mix m; 10 ms frames of 480 samples, k frames):
- When k >= 2: frame dB; active frames within 50 dB of the max. `length` = (last active - first active + 1) x 0.01 s
  (3 decimals). `peak_frac` = (peak frame - first active + 0.5) / (active span in frames) (3 decimals).
- attack, t20, t40 from the search module's envelope_stats of m (part 7), 1 decimal.
- sh150, sh1k, sh4k, sh12k, shhi = the tone shares of m in the 5 regions.
- corr: for stereo, the correlation of the mean-removed channels (3 decimals; 1.0 when a channel is constant); mono
  1.0.

`study_penalties(sm, targets)`: for each target key with a measure:
- share keys: p = min(2, max(lo - v, v - hi, 0) / 6), part tone, text `<region> share <v:+.1f> dB (target lo..hi)`.
- corr: p = min(1.5, distance / 0.3), part polish, text `stereo corr <v:.2f> (target lo..hi)`.
- peak_frac: p = min(1.5, distance / 0.25), part env, text `peak at <v as %> of the sound (target lo%..hi%)`.
- length (s) and attack, t20, t40 (ms): in log2 octaves outside the range: floor fl = 1e-3 for length, 1.0 for ms
  keys; vv = max(v, fl), lo' = max(lo, fl), hi' = max(hi, lo, fl); p = min(1.5, max(log2(lo' / vv), log2(vv / hi'),
  0)); part env; text `<key> <v><unit> (target lo..hi)`.
- Record off[key] (3 decimals) and add to the part when p > 0.
- Caps: tone 3.0, env 2.0, polish 1.5.
- Words: `study targets: <texts>` or `study targets met`.

### `total_v2(r, W, mode)`

Pure function of a row. W = W2 overlaid by W. real weight = W.real in real mode, else 0.
total = W.match min(match_z, 6) + real (2 p_real - 1) + W.typ clip(typ, -3, 2) (0 when None) - W.feat feat_pen -
W.alt max(0, 0.5 - alt_margin) - W.tone tone_pen - W.env env_pen - W.art art_pen - W.polish polish_pen (missing
penalties are 0).
passes = match_z >= 3 and alt_margin > 0 and (mode is not real or p_real >= 0.77) and (typ None or typ >= -2) and
feat_pen <= 1 and no hard artifact and env_pen <= ENV_PASS and polish_pen <= POLISH_PASS and not tone_far.

Why each weight: match 1, real 2, typ 0.5, feat 0.7, alt 1 come from the first version (calibrated on the first
forge test; the model checks saturate after a search, so they mostly decide early rounds). tone 1.0 (the first
version recommended 1.5 when tone was the only model-free check; the envelope now carries part of the same evidence,
so 1.0 avoids counting it twice). env 1.0 (one point per typical ref-to-ref spread of excess plus 1 per 6 dB beyond
every ref, the same scale as tone; the shape over time is what a blind listener heard in a forged impact). art 1.0
(each detector is capped at 2, so one blatant defect costs as much as the whole realness term; hard defects also
fail the pass). polish 0.75 (relative to ranges that 3-6 refs pin down loosely).

### Summary

`PASS ` when passing, then `total <t:.2f>: ` and these parts joined by `; `:
1. `match z <z:+.2f>` plus ` (best alternative <alt:+.2f>)` when an alternative exists.
2. `p_real <p:.2f>` plus ` (not required: designed)` in designed mode.
3. `typicality vs refs z <typ:+.2f> (nearest <ref>)` when typ exists.
4. `outside the refs: <key> <dev:+.1f>, ...` for feature devs beyond 1.5.
5. `sounds like: <terms>`.
6. The tone words, the envelope words, the polish words, and the study words when the study has off-target keys.

### `score_refs(loo=True)`

Scores every ref with the same critic. With loo, ref i is compared with the other refs only (its typicality,
feature spread, tone, envelope and polish ranges come from the others), so a real recording is not its own nearest
neighbour. With fewer than 3 other refs, ref_typ_z and env_ratio are None. Memoised per critic by (loo, sorted
weights, mode, version, identity of extra, refs); return a deep copy of the memo.

### `consistency(xs)`

`loudness_spread(xs)` (part 4), plus `refs` = the refs' loudness spread when there are refs.

### `holdout(xs, cache=True)`

Checks that are not in the objective. They stay held out only while nobody picks or steers by them.
For each sound: the audio tagger's probabilities of the mono mix; `audioset_top` = the top 3 (label, probability)
pairs; `audioset_synth_max` = the max probability over the tagger's synth classes that exist in its label index (3
decimals). Cache: key = `<the tagger model's cache name>/<sha1 of the mono float32 samples>`; in memory and as JSON
files under `<HARK_HOME>/emb/forge_holdout/`. A cached `audioset_top` list of lists becomes a list of tuples. Writes
are atomic (temporary file then replace) and ignore OS errors. Return copies.

## Constants

STUDY_TARGETS (category -> key -> (lo, hi)); keys: length s, attack / t20 / t40 ms, peak_frac, share keys in dB of
the total, corr:

| category | targets |
|---|---|
| pro_whoosh_big | length 0.5-1.2, attack 100-350, t20 60-160, t40 180-400, sh150 -1.5-0.0, corr 0.8-1.0 |
| riser | length 1.0-6.0, peak_frac 0.7-1.0, t40 0-400, corr 0.5-1.0 |
| pro_riser | length 0.8-2.5, peak_frac 0.75-1.0, t40 0-300, corr 0.8-1.0 |
| impact | length 1.0-8.0, t40 1000-3000, sh150 -2.5 to -0.2 |
| pro_impact | length 0.6-1.3, attack 40-300, t40 250-1100, sh150 -1.25-0.0, corr -0.1-0.6 |
| sub_drop | length 0.8-3.0, attack 0-450, sh150 -1.0-0.0, corr 0.8-1.0 |
| pro_sub_drop | length 0.8-1.6, attack 250-460, t40 600-1600, sh150 -0.7-0.0, corr 0.8-1.0 |
| logo_sting | length 0.8-2.5, t40 250-1200, corr 0.6-1.0 |
| pro_logo_sting | length 0.5-1.5, attack 0-90, t20 30-110, t40 150-450, corr 0.9-1.0 |
| glitch | length 0.2-1.5, sh150 -200 to -10, corr -0.3-0.6 |
| pro_glitch | length 0.3-1.0, sh150 -200 to -12, sh12k -8-0, corr -0.3-0.4 |
| pro_shimmer | length 1.4-2.6, attack 150-800, sh150 -200 to -30, corr -0.2-0.45 |
| pro_pop | length 0.05-0.2, attack 0-60, t20 3-30, corr 0.9-1.0 |
| pro_tap | length 0.06-0.2, attack 2-15, t20 10-30, t40 18-50, corr 0.85-1.0 |
| pro_click | length 0.02-0.08, attack 0-4, t20 4-20, sh150 -200 to -20, sh12k -10-0, corr 0.9-1.0 |
| pro_typing_run | sh150 -200 to -25, sh1k -200 to -20, corr 0.9-1.0 |
| pro_zip and zip | length 0.12-0.4, attack 10-50, corr 0.85-1.0 |
| downlifter | length 1.0-4.0, attack 0-80, t40 500-3500, corr 0.3-1.0 |

Other numbers: realness pass 0.77; pure-tone cap 0.03; match cap 6; typicality clip [-3, 2] and pass -2; feature z
beyond 1.5, feat pass 1.0; alt margin soft limit 0.5; no-alternative margin 9.0; typicality neighbours k = 3 and at
least 3 refs; tone far factor 1.6 with a tone target.

## Data assets

STUDY_TARGETS is a table of numbers. DERIVE: the clean side keeps these numbers as facts (they are measurements of
professional films, not code). A re-measurement needs the study films (NEEDS-DATA); the fallback is to ship the table
as given.

## Errors and edge cases

- A silent or empty sound: total -inf, passes false, summary `silent`.
- No refs: mode auto gives designed; no tone, envelope, typicality or feature terms; polish uses absolute limits; the
  study fallback applies when the category has targets.
- 1 or 2 refs: no typicality (needs 3), no tone ratio, no env ratio; the study fallback fills in tone and envelope
  penalties.
- Scoring a single ref leave-one-out: the tone and envelope checks return `{}` and are skipped.
- The model extras missing (no ML install): the listen unit's prompt to install applies; the critic cannot run
  without its models. The model-free parts (Prescreen, ToneRef, EnvRef, PolishRef) run without them.

## Acceptance tests

C1. `resolve_mode('auto', 'a glitchy digital stutter', ['r.wav'])` = `designed`; `resolve_mode('auto', 'a wooden
    knock', [])` = `designed`; with refs and a plain target = `real`; `resolve_mode('real', ..., [])` = `real`.
C2. `total_v2` of the row {match_z 4, alt_margin 1, p_real 0.9, ref_typ_z 1, feat_pen 0.5, tone_pen 0.2, env_pen 0.3,
    art_pen 0, polish_pen 0.4} in real mode = 4 + 2 (0.8) + 0.5 - 0.35 - 0 - 0.2 - 0.3 - 0 - 0.3 = 4.95, and passes
    true. In designed mode the realness term is 0 (total 3.35), and p_real 0.5 still passes.
C3. A row with `hard` = ['clicks'] never passes.
C4. `study_penalties({'length': 0.4}, {'length': (0.8, 3.0)})` gives env 1.0 (one octave short); with length 0.1 it
    gives 1.5 (capped); `{'corr': 0.2}` against (0.8, 1.0) gives polish 1.5 (0.6 / 0.3 = 2, capped at 1.5).
C5. `widen_targets({'t40': (100, 200)}, [{'t40': 50}, {'t40': 300}])` = {'t40': (50, 300)}.
C6. `score_refs()` on 4 refs: each row has `left_out` set to its own basename and a non-None ref_typ_z; on 2 refs every
    ref_typ_z is None.
C7. Two calls of `score_refs()` return equal rows and the second call does no embedding (memo).
C8. `holdout` of the same samples twice reads the cache the second time (no tagger call).
C9. With `extra=lambda x, row: -1.0`, every total is 1.0 lower and `extra` is -1.0.
C10. A category with 1 ref and study targets adds `study` to every row; a category with 4 refs never does.
