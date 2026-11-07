# forge-core part 7: the search loop, the pre-screen and the seed bank

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.search`: parameter encoding, start-point
filling, rendering on threads, the model-free descriptor and pre-screen, the optimiser (an evolution strategy and a
separable CMA-ES with restarts), robust re-renders, diverse picks, and the seed bank.

## Interface

| name | kind | meaning |
|---|---|---|
| `SR` | int | 48000 |
| `SEEDS_DIR` | path | the package folder `hark/forge/seeds/` (committed seed banks) |
| `MidRangeFill` | warning class, subclass of `UserWarning` | a start key got the spec's mid-range because no DEFAULTS had it |
| `midrange(spec)` | function | the value that unit coordinate 0.5 decodes to |
| `recipe_defaults(recipe)` | function | the fallback values for missing start keys |
| `fill_start(space, start, defaults, own=None)` | function | `(full start dict, notes)` |
| `render(recipe, params, seed)` | function | float32 render peak-normalised to -1 dBFS, or None |
| `render_many(recipe, jobs, workers=None)` | function | list of renders for `[(params, seed), ...]` |
| `ENV_KEYS` | tuple | `attack_ms, t10_ms, t20_ms, t40_ms, centroid_ms, crest_db, end_db` |
| `envelope_stats(x, hop_ms=2.0)` | function | dict of ENV_KEYS |
| `artifacts(x)` | function | `{'flags': {...}, 'penalty'}` |
| `describe(x)` | function | `{'profile', 'env', 'art'}` |
| `sound_dist(a, b)` | function | float distance of two descriptors |
| `Prescreen(refs=None, tone_target=None, env_w=0.5, env_keys=('t40_ms', 'crest_db', 'end_db'), polish_ref=None, polish=True, study=None)` | class | model-free ranking |
| `prescreen_for(critic)` | function | a Prescreen from a critic's refs, tone target, polish ref and study targets |
| `optimize(recipe, critic, budget=160, pop=16, elite=4, seed=0, init=None, sigma0=0.22, sigma1=0.04, fresh=0.15, log=None, space=None, reps=1, strategy='es', prescreen=0, workers=None, patience=4, sigma_stop=0.03, pre=None, cma=None)` | function | the search result |
| `robust(recipe, critic, params, seeds=4, seed0=5000, workers=None)` | function | robust stats |
| `diverse(results, recipe, k=4, min_dist=0.08, min_sound=2.0)` | function | up to k results |
| `seeds_path(recipe, folder=None)` | function | `<folder or SEEDS_DIR>/<recipe>.json` |
| `load_seeds(recipe, target=None, refs=None, n=6, folder=None)` | function | list of param dicts |
| `save_seeds(recipe, picks, target=None, refs=None, source='', folder=None, per_context=6, cap=48, delivered=None)` | function | the bank dict |
| `report_members(report)` | function | a report's picks as save_seeds picks |
| `pin_delivered(recipe, report, category, target=None, refs=None, folder=None, source=None)` | function | the bank dict |

The worker count: `workers` when given, else the environment variable `HARK_THREADS`, else 2; clamped to [1, 8].

## Behaviour

### Parameter encoding

Each SPACE key maps to a unit coordinate in [0, 1], in SPACE order.
- Decode (clip the coordinate into [0, 1] first): choices: element min(len - 1, int(v len)); log: exp(ln lo + v (ln hi
  - ln lo)); int: round(lo + v (hi - lo)); linear: lo + v (hi - lo).
- Encode: a missing value gives 0.5; a choice gives (index + 0.5) / len, or 0.5 when the value is not in the list;
  log: (ln max(v, 1e-12) - ln lo) / (ln hi - ln lo); linear and int: (v - lo) / (hi - lo). Clip to [0, 1].
- Inside test: a choice is inside when the list holds it; a range when lo <= v <= hi and, for int, v is integral. A
  type error means outside.
- Clip to a numeric spec: the nearest value in [lo, hi], rounded for int; None for a choice list or a non-numeric
  value.
- `midrange(spec)` = the decode of 0.5.

### `recipe_defaults(recipe)`

own = the object's DEFAULTS. Find the module that defines the object's `render` (unwrap a partial). When that module
is not the object itself and has a dict SPACE and a dict DEFAULTS, return module DEFAULTS overlaid by own. Else own.
So a stand-in object (for example a family member whose DEFAULTS hold one preset, or the object of a driver's final
pass) still gets the recipe module's DEFAULTS.

### `fill_start(space, start, defaults, own=None)`

own defaults to `defaults`. For each SPACE key:
1. v = the start's value, source `start`. When missing, v = defaults[key], source `DEFAULTS` when own has the key,
   else `module DEFAULTS`.
2. Still missing: full[key] = midrange(spec); note `mid-range`.
3. v outside the spec: to = the clip of v; for a choice not in the list, to = midrange (the middle entry: the list is
   the author's narrower choice). Source `start`: note `start clipped` with `was`, and leave the start value as is
   (the search scores the decoded, clipped value). Else full[key] = to, note `DEFAULTS clipped` with `was`.
4. Else full[key] = v; note `module DEFAULTS` when that was the source.
A note is `{'key', 'value', 'from'[, 'was']}`. Returns (full, notes).

Reason: before this rule, a missing key always got mid-range, and a family pass once received a hidden 0 dB onset
tick that way.

### `render(recipe, params, seed)`

Call `recipe.render(copy of params, rng(seed))`. On an exception, write one line to stderr the first time per
recipe name: `forge: recipe <name> raised <Type>: <msg> (<file>:<line>); later failures of this recipe are not
printed`; return None. Return None for an empty output, a non-finite value, or a peak under 1e-7. Else normalise to
-1 dBFS and return float32. Level never decides a score.

`render_many`: same order and same samples as a serial loop, because each job owns its seed. Thread pool when the
worker count is above 1 and there are at least 2 jobs.

### Model-free descriptor

`envelope_stats(x, hop_ms=2)` on the mono sum, frames of n = 96 samples, k = floor(len / n):
- k < 2: every key 0.0, except end_db -90.0.
- Frame power pw and dB e = 10 log10(pw + 1e-18). Peak frame ip and level pk. Onset on = the first frame with
  e > pk - 30.
- attack_ms = (ip - on) hop + hop / 2.
- t10, t20, t40: frames from the peak until e first falls more than 10, 20, 40 dB under pk (or k - ip), times hop.
- centroid_ms: the power-weighted mean frame index from the onset, times hop, plus hop / 2.
- crest_db: 20 log10(peak sample / RMS of the samples from the onset whose frame is within 40 dB of the peak); 0.0
  when there are none.
- end_db: max(-90, 10 log10(mean pw over the last quarter of frames) - pk). A noise-floor plateau shows where real
  edits fade to silence.

`artifacts(x)` (cheap checks; the fallback when the polish module is not available) on the mono sum, pk = peak +
1e-12:
- flat_top: samples with |x| > 0.98 pk whose step from the previous sample is <= 1e-6 pk. When at least 4 such
  samples and the longest run is >= 4: flag the longest run, +1.0.
- dc: |mean| / RMS > 0.1: flag mean / RMS (3 decimals), +0.5.
- start_jump: |x[0]| / pk > 0.05: flag it, +0.5.
- cut_end: the RMS of the last 5 ms re pk above -45 dB: flag the dB value, +1.0.
- gap: runs of exact zeros that do not touch either end and are longer than 5 ms: flag the count, +1.0.
- ultrasonic: energy share above 20 kHz above -25 dB: flag the value, +0.5.

`describe(x)` = `{'profile': tone profile (part 3), 'env': envelope_stats, 'art': artifacts}`.

Envelope vector: ln(max(v, 0.5)) for attack, t10, t20, t40, centroid; then crest_db / 6 and end_db / 6 (missing:
0 and -90).

`sound_dist(a, b)` = RMS of the profile difference + 4 x RMS of the envelope vector difference. So one e-fold of
timing difference or 6 dB of crest or end level counts like 4 dB of tonal balance.

### `Prescreen`

Ranks sounds against refs by measurement only, cheap enough for many renders before the critic embeds a few.

Construction:
1. Load refs (paths) or take arrays.
2. When `study` targets are given and there are fewer than 3 refs: study = the critic's widened study targets
   (part 8: `widen_targets(study, [study_measures(ref) ...])`).
3. Polish reference: `polish_ref` when given; else, when `polish` is true, a PolishRef of the refs (an error leaves
   none, and the built-in artifact checks rank alone).
4. Descriptors of all refs. With refs: median profile; spread = max(median leave-one-out distance, 0.5) with 3 or
   more refs, else 4.0; a given tone_target replaces the spread. Envelope vectors: median, and a robust sd =
   max(IQR / 1.35, floor) with floor 0.2 for the 5 log times and 2/6 for the 2 dB keys.

`row(x, polish=True)`:
1. d = describe(x). art = d's artifact penalty.
2. With polish and a polish ref: check x; record `polish` = {art_pen, polish_pen, hard}; art = art_pen + 0.75
   polish_pen + 1.0 x len(hard) (the critic's weights). An error keeps the cheap art.
3. With study targets: penalties of the study measures (part 8); record `study` = the off-target list; art += tone +
   env + 0.75 polish penalties.
4. With refs: tone_db = RMS(profile - median) (2 decimals); tone_ratio = tone_db / spread (3 decimals); env_z per key
   = (vector - median) / sd (2 decimals); env_pen = mean over env_keys (default t40, crest, end) of max(0, |z| -
   1.5) (both signs count, so a tail that is too short is penalised too); pre = -(tone_ratio + env_w env_pen + art).
5. Without refs: pre = -art.
6. The row holds `art`, `desc`, and the keys above. Higher pre is better.

`score(xs, polish=True)`: a row per sound; a None sound gives `{'pre': -inf, 'art': {'flags': {'silent': 1},
'penalty': 9.0}}`.

`nearest(desc)`: the ref index with the smallest sound_dist, or None without refs.

Measured (source): without polish, the pre-screen's rank correlation with the critic was 0.60-0.63 on a key-press
recipe with 96 candidates; with polish, against critic v2 on 72 candidates, 0.53 -> 0.69 (whoosh) and 0.60 -> 0.71
(key press); picking 12 of 36 by pre-screen raised the mean critic total of the picked set in every half tested.

### `optimize`

Result: `{'best': the top max(elite, 8) finite results, 'history', 'generations', 'gens', 'timing', 'strategy',
'prescreen' (K, or 0 when off), 'restarts', 'full_space', 'fill_warnings'}`. A result is `{'gen', 'u' (unit vector),
'params', 'seed', 'score' (the critic row), 'desc'?, 'pre'?}`.

Setup:
1. base = the recipe's DEFAULTS (keys outside the searched space keep these). space = the given space, else the
   recipe's SPACE; `full_space` says which.
2. One generator R = default_rng(seed) drives every search draw.
3. Starts: DEFAULTS first, then each init dict overlaid on DEFAULTS. Fill each start with fill_start(space, start,
   recipe_defaults(recipe), own=DEFAULTS). Every note not from `module DEFAULTS` becomes a fill warning
   `{'start': j, ...note}`. When warnings exist: log one message `forge: <n> start value(s) not from the start or
   DEFAULTS: start <j> <key> = <value> (<from>); ...` (first 8, then ` ...`); when any is `mid-range`, also raise a
   MidRangeFill warning (heard even when the log is silenced).
4. gens = max(1, budget // pop). K = prescreen; the pre-screen is used when K > pop. Build it from the critic when
   none was passed.
5. CMA options: sigma0 0.15, mu 0.5, elite 0.0, overridden by `cma`. A CMA state starts at the encoded first start.

Per generation g:
1. n_new = K with the pre-screen, else pop.
2. Candidates:
   - CMA: in generation 0, the encoded starts (at most pop) come first and count as fixed. Then ask the CMA for the
     rest. Then draw one seed per candidate (integers in [0, 2^31)).
   - ES: step sig = sigma0 (sigma1 / sigma0)^(g / max(1, gens - 1)). For each of n_new: in generation 0, the encoded
     start j when j < number of starts and j < pop (fixed), else a uniform vector; later, a uniform vector with
     probability `fresh` (or when there are no elites), else a parent = elites[min(len - 1, integer in [0,
     max(1, min(elite, len)))) ] plus normal noise of sd sig, clipped to [0, 1]. Draw each candidate's seed right
     after the candidate. Draw order per candidate: the uniform test (generation > 0), then the vector or the parent
     index and noise, then the seed.
3. params = base overlaid by the decode of each vector. For fixed starts, overlay the start's own values for keys
   outside the space and for keys whose value is inside the space (starts are scored exactly as given).
4. With the pre-screen: render all K candidates, pre-screen them (with polish), order = the fixed ones first, then the
   rest by falling pre (ties by index). Keep the first pop. Keep their renders as the first renders.
5. Evaluate (below).
6. When the pre-screen did not run: describe the first renders (pre-screen rows without polish when a pre-screen
   object exists, else plain descriptors) for diversity and the report.
7. A history row per candidate: the critic row, or `{'total': -inf, 'passes': false, 'summary': 'render failed or
   silent'}` when it has none. Add `desc`. When the pre-screen has refs and the row has `pre`, add `pre` = {pre,
   tone_db, tone_ratio, env_pen, art (flags), polish?}.
8. Elites = the best `elite` finite totals over the whole history.
9. Generation stats: gen (1-based), best (the best elite total), gen_best, gen_median (over finite totals of this
   generation), passes, scored, rendered (K with the pre-screen, else the rows), secs since the start, sigma (ES
   step, or the CMA sigma), best_tone_db (when the best has tone_db), restart (CMA).
10. CMA: tell the state the finite candidates and totals. When the best elite total rose by more than 1e-3, reset the
    stale count; else count up. When not the last generation and (sigma < sigma_stop or stale >= patience):
    restart. Record the current mean as tried. Candidates `far` = the top 24 ranked results whose unit vector is more
    than 0.2 sqrt(d) from every tried mean. On odd restarts with a far candidate, restart from the first far
    candidate (`elite`), else from a uniform random point (`random`). Reset the step to sigma0. stale = 0.
11. Log line (when log is given and elites exist): `gen <g>/<gens>: best total <t:.2f> (PASS | no pass) z <match_z>
    p_real <p> typ <typ> pen <feat_pen>`, plus sigma, restart, tone dB, and `[<K> rendered, <pop> scored]` with the
    pre-screen.

Evaluate (candidates, seeds, reps, first renders):
- reps = max(1, reps). Jobs (j, k, seed_j + 7919 k) for k < reps. Render the jobs that have no first render (k > 0,
  or all without a pre-screen). Score all live renders in one critic call.
- A candidate with a failed rep is dropped (unreliable). With reps > 1: total = the mean of the rep totals (-inf when
  any is not finite), `total_reps` = the list, `passes` = at least half of the reps pass; other keys from the first
  rep.
- Timing: render_s, score_s, rendered, scored.

Timing totals: render_s, pre_s, score_s (2 decimals), rendered, scored, total_s, scored_per_min and
rendered_per_min (1 decimal).

Design note (source measurements on a key-press recipe with 2 render threads): at 96-240 scored candidates CMA-ES
did not beat the ES. On a model-free proxy objective, ES -1.23 vs CMA -1.26 to -1.36 at 96, and -1.136 vs -1.154 at
240. With the real critic (2 seeds, 96): best 6.55 vs 6.03, top-5 mean 5.77 vs 5.30. Elitist CMA and CMA with a small mu
scored lower still. The gain comes from the pre-screen: ES with a 48-render pre-screen raised the passing candidates from
10.5 to 36.5 of 96 and the top-5 mean from 5.77 to 6.17 (CMA with the pre-screen: 25.5 and 5.92). The text-audio
model is about 70 % of a generation's time; the measurements cost about 10 ms a sound (45 ms with polish).

### Separable CMA-ES

State in the unit cube with a diagonal covariance C (Ros and Hansen 2008), dimension d, lambda = pop.
- mu = max(1, round(lambda x mu_frac)). Weights w_i = ln(mu + 0.5) - ln i, normalised. mueff = 1 / sum w^2.
- cs = (mueff + 2) / (d + mueff + 5); ds = 1 + 2 max(0, sqrt((mueff - 1) / (d + 1)) - 1) + cs;
  cc = (4 + mueff / d) / (d + 4 + 2 mueff / d); with sep = (d + 2) / 3: c1 = min(1, 2 sep / ((d + 1.3)^2 + mueff));
  cmu = min(1 - c1, 2 sep (mueff - 2 + 1 / mueff) / ((d + 2)^2 + mueff));
  chi = sqrt(d) (1 - 1 / (4d) + 1 / (21 d^2)).
- Reset: mean m, step sigma, C = ones, paths ps = pc = 0, g = 0.
- Ask n points: draw an (n, d) normal matrix; a share `elite` of the points centre on the elites so far (cycled),
  the rest on m; point = mirror(centre + sigma sqrt(C) z). Mirror folds any real number into [0, 1]: r = x mod 2;
  r > 1 -> 2 - r.
- Tell (points U, totals, higher is better): take the best mu by a stable sort; Y = (U - m) / sigma; clip each Y row
  so its C-scaled norm is at most 2 chi (Hansen 2011, against injected and mirrored outliers); yw = the weighted sum
  (weights renormalised over the rows used); m = clip(m + sigma yw, 0, 1); g += 1;
  ps = (1 - cs) ps + sqrt(cs (2 - cs) mueff) yw / sqrt(C);
  hs = |ps| / sqrt(1 - (1 - cs)^(2g)) < (1.4 + 2 / (d + 1)) chi;
  pc = (1 - cc) pc + hs sqrt(cc (2 - cc) mueff) yw;
  C = (1 - c1 - cmu) C + c1 (pc^2 + (1 - hs) cc (2 - cc) C) + cmu sum(w Y^2), clipped to [1e-4, 16];
  sigma = clip(sigma exp((cs / ds)(|ps| / chi - 1)), 1e-3, 0.6).

### `robust(recipe, critic, params, seeds=4, seed0=5000, workers=None)`

Seeds = seed0 .. seed0 + seeds - 1 (or the given list). Render all, score the live ones. Return `{'n', 'seeds',
'totals' (finite, 3 decimals), 'failed', 'mean', 'min', 'max', 'sd' (population; all None without a finite total),
'pass_rate' (passing / n, 3 decimals), 'tone_db_mean' (when rows have tone_db)}`. The best single render of a search
is a winner's curse: judge finals by their robust mean.

### `diverse(results, recipe, k=4, min_dist=0.08, min_sound=2.0)`

1. Order: passing first, then by falling total. Drop non-finite totals unless all are non-finite.
2. Parameter distance = |u_a - u_b| / sqrt(len(u)).
3. A result is accepted when, for every accepted one, it is not the same object, the parameter distance is at least
   min_dist, and (in the sound pass, when both have `desc`) the sound distance is at least min_sound.
4. Pass 1 with the sound test, pass 2 without (only when min_sound is not 0): add results in order until k.
5. Return the accepted results sorted again: passing first, then by falling total.

### Seed bank

A seed bank file `<recipe>.json`: `{'recipe': name, 'entries': [...]}`. An entry: `params` (floats rounded to 6
significant digits), `rank_score` (3 decimals, or null for a pinned entry without a finite score), `context`
(`<target> | <sorted comma list of ref basenames>`), `total`, `tone_db`, `passes`, `source`, optional
`robust_mean`, `robust_pass_rate`, `delivered` (a tag), `was_delivered`, `member`, `seed`.

`load_seeds(recipe, target, refs, n=6, folder)`: [] when the file is missing. Sort entries by: same context first,
then delivered first, then by falling rank (a missing or non-finite rank counts as -1e9). Return up to n param
dicts.

`save_seeds(recipe, picks, target, refs, source, folder, per_context=6, cap=48, delivered=None)`:
1. Load the bank or start an empty one. ctx = the context text. tag = ctx when delivered is True, the text of
   delivered when it is given, else None.
2. For each pick: rank = robust mean when present, else score total. Skip a pick without a finite rank unless tagged.
   Build an entry; with a tag add `delivered`, `member` (family, preset, member or name) and `seed`.
3. With a tag, entries pinned earlier under the same tag lose `delivered` and get `was_delivered` (a re-delivery
   replaces the delivery; old members stay as ordinary entries while they rank).
4. All entries = new + old. Pinned = those with `delivered` (always kept, outside per_context and cap, never merged).
5. For each context in first-seen order: the context's pinned entries are seeds of a de-duplication list; add its
   unpinned entries by falling rank, skipping exact parameter duplicates, until per_context added.
6. entries = pinned + the kept ones, cut so the total of kept is at most cap - pinned (newest contexts first, because
   new entries come first).
7. Write atomically (temporary file `<path>.tmp<pid>`, then replace), indent 1. Create the folder when missing.

Reason for pins: without them, attempts that scored higher but were never delivered could push a delivered member
out of the bank (one category kept 1 of its 5 delivered members).

`report_members(report)`: for each pick, a dict of its `score`, `robust`, `seed`, `family`, `preset`, `member`,
`name` (present ones), with `params` = the pick's `family_params` overlaid by its `params`, and `score` default {}.

`pin_delivered(recipe, report, category, target, refs, folder, source)`: load the report when it is a path; save its
members with delivered = category, target and refs from the report unless given, source `delivered final
(<category>)` plus the path.

## Constants

| item | value |
|---|---|
| render peak | -1 dBFS; silent under 1e-7 |
| worker default and cap | HARK_THREADS or 2; 1..8 |
| envelope hop | 2 ms; onset -30 dB; crest window 40 dB; end = last quarter, floor -90 dB |
| envelope sd floors | 0.2 (log times), 2/6 (dB keys / 6) |
| pre-screen spread | median LOO, floor 0.5; 4.0 with fewer than 3 refs |
| env penalty | z beyond 1.5, weight env_w 0.5, keys t40, crest, end |
| polish weights in pre | art_pen + 0.75 polish_pen + 1.0 per hard |
| sound_dist | profile RMS + 4 x envelope RMS |
| cheap artifacts | flat top 0.98 pk, 1e-6 step, 4 run; dc 0.1; start 0.05; end -45 dB over 5 ms; gap 5 ms; ultrasonic -25 dB above 20 kHz; penalties 1.0 / 0.5 / 0.5 / 1.0 / 1.0 / 0.5 |
| ES | elite 4, fresh 0.15, sigma 0.22 -> 0.04 geometric |
| reps seed step | 7919 |
| CMA | sigma0 0.15, mu 0.5, elite 0, patience 4, sigma_stop 0.03, sigma clamp [1e-3, 0.6], C clamp [1e-4, 16], restart distance 0.2 sqrt(d) over the top 24 |
| best list | max(elite, 8) |
| robust | 4 seeds from 5000 |
| diverse | min_dist 0.08, min_sound 2.0 |
| seed bank | n 6, per_context 6, cap 48, 6 significant digits |

## Data assets

`hark/forge/seeds/<recipe>.json`: seed banks. REGENERATE: our own forge runs write them (`save_seeds`,
`pin_delivered`). Ship empty or absent banks at first: `load_seeds` returns [] and the search starts from DEFAULTS.
The source banks partly held parameters tuned against references that are benchmark references now (a soft leak), so
starting fresh is also cleaner.

## Errors and edge cases

- A recipe exception is printed once per recipe to stderr; the candidate scores -inf with the summary `render failed
  or silent`.
- A candidate with a failed rep is dropped from scoring.
- A missing start key with no DEFAULTS gets mid-range and raises a MidRangeFill warning; `fill_warnings` lists it.
- With no refs, the pre-screen ranks by artifacts only, and `nearest` is None.
- `budget < pop` still runs 1 generation.
- `diverse` with fewer distinct results than k returns fewer than k.

## Acceptance tests

R1. Decode(encode(p)) = p for a log, linear and choice key; int keys round. Encode of a missing key is 0.5.
R2. `fill_start({'a': (0, 1)}, {}, {})` gives a = 0.5 with a `mid-range` note; with defaults {'a': 2.0} it gives 1.0
    with a `DEFAULTS clipped` note and was 2.0.
R3. `optimize` with a start that lacks a SPACE key and no DEFAULTS for it emits a MidRangeFill warning.
R4. `optimize(rec, critic, budget=32, pop=8, seed=1)` twice gives identical histories (params, seeds, totals); the
    same with workers 1 and workers 4.
R5. Generation 0 scores DEFAULTS exactly (the first history row's params equal DEFAULTS for keys inside the space).
R6. With prescreen 24 and pop 8, each generation renders 24 and scores 8, and `timing.rendered` = 24 gens.
R7. With reps 2, each history total is the mean of its `total_reps`.
R8. `robust` with seeds 4 returns n 4 and seeds [5000, 5001, 5002, 5003].
R9. `diverse` never returns two results whose parameter distance is under 0.08.
R10. `save_seeds` twice with 10 picks of the same context keeps 6 entries; with delivered='cat' all 10 are kept and
     pinned; a second delivery under 'cat' unpins the first set (`was_delivered`).
R11. `load_seeds` puts same-context delivered entries first.
R12. A CMA run with patience 1 on a flat critic restarts at least once and records `restart` in the generation stats.
