# forge-core part 11: the blind listening test

Index: [forge-core.md](forge-core.md). This part covers the script `eval/forge/blind_v2.py`. Part 10 covers the
benchmark, the refset build and the target numbers.

Builds a clean A/B listening test: forge finals against held-out real recordings of the same category. It also builds
a "progress" listening file (first version against second version). Nothing is synthesised: every sound is an existing
file (forge finals, first-version finals, bench refs), only trimmed, faded, gain-changed and placed on silence. There
are no markers, beeps or voice. Trials are separated by silence.

Main rule: nothing but the sound itself may give the answer away. A real recording carries a room floor and a made
sound sits on digital silence, so neither clip may play any floor before or after its sound. Both clips must play
equally loud at their loudest moment.

CLI: `python eval/forge/blind_v2.py [--out DIR] [--n 20] [--seed S] [--dry-run] [--allow-fail] [--progress DIR]`.
Default out `out/showcase/blind_v2`. `--dry-run` checks cleanliness and writes nothing. After listening:
`python <out>/score_answers.py A B B A ...`.

## Inputs

- Made finals: `<repo>/out/forge_v2/<category>/final/<category>_NN.wav` (two digits).
- Bench refs: `refsets.refset(cat, 'bench')` (part 9) and the refset manifest `out/refs/manifest.json` (part 10, section 10.3) for each
  ref's source path, trim, fade out and gain.
- Audio load: read as float64, 2-D. Resample to 48 kHz with a polyphase resampler when needed. Mono becomes 2 equal
  channels. Keep the first 2 channels. A start and stop time read only that range.

## Constants

| name | value | meaning |
|---|---|---|
| LEAD, GAP, BETWEEN, TAIL | 1.0, 1.2, 2.5, 1.0 s | silence before trial 1, between A and B, between trials, after the last |
| COMPAT | 1.3 | a made final is length-compatible when its audible extent is within this factor of an available bench ref |
| ON_DB, END_DB | -50, -50 dB | the lowest onset and end levels |
| REACH_MARGIN, FLOOR_MARGIN, HEAD_MARGIN | 3, 10, 1 dB | margins added to reach, floor and head |
| LEVEL_CAP | -6 dB | onset and end levels never go above this |
| FADE_IN_S | 0.004 s | the raised-cosine fade in before the onset |
| FADE_DB, FADE_MIN_S | 6 dB, 0.005 s | the fade out covers the last 6 dB of decay above the end level, at least 5 ms |
| REF_FADE_DEFAULT, MADE_FADE_GUARD | 0.03, 0.01 s | a bench ref's end fade when the manifest has none; a made final's end guard |
| ENV | short: hop 2.5 ms, smooth 3 frames; long: hop 5 ms, smooth 6 frames | the trim envelope per pair class |
| CLEAN_HEAD, CLEAN_REACH, CLEAN_FLOOR | -30, -30, -50 dB | pick preference: quiet start, natural decay, low floor |
| SHORT_S | 1.0 s | a pair is long when the longer audible extent is at least this |
| WIN | short 0.1 s, long 0.4 s | the loudness window of the pair measure |
| TARGET | short -15 LUFS, long -16 LUFS | the level the pair is set to |
| TP_MAX | -1.0 dBTP | the true-peak ceiling |

FAIL limits: loudest-moment difference 0.5 LU; true peak above TP_MAX + 0.05; floor margin under 6 dB; pre-roll above
5 ms; trim edges that differ by more than 10 dB while the louder is above -20 dB; floor heard for more than 100 ms
inside a clip at above -70 dBFS.
WARN limits: length ratio 1.6; true-peak difference 6 dB; other-window difference 2 LU; mono corr 0.995 with the other
clip under wide corr 0.9; a floor above -60 dBFS as played; floor heard for more than 30 ms.

PLAN (category, answer-sheet name, trials at N = 20, tier). The tier comes from the round 3 and round 4 benchmark
verdicts:

| category | name | trials | tier |
|---|---|---|---|
| whoosh_big | big whoosh | 2 | strong |
| whoosh_fast | fast whoosh | 2 | strong |
| click | UI click | 2 | strong |
| typing_key | key press | 2 | strong |
| shimmer | shimmer | 2 | strong |
| pop | pop | 2 | strong |
| swipe | cloth swipe | 1 | strong |
| swish | swish | 1 | strong |
| impact | impact | 1 | strong |
| hover_tick | hover tick | 1 | middle |
| tap | tap | 1 | weak |
| toggle | toggle | 1 | middle |
| notify | notification ding | 1 | weak |
| riser | riser | 1 | weak |

Weak categories are in the test on purpose, so the test is honest.

Excluded categories and why (written to the key): logo_sting (its bench refs are a triangle hit and a struck ring, the
same files as the notify bench, so a pair would test form, not realism); downlifter and glitch (no bench refs);
sub_drop (one bench ref, a designed tom pitched down 24 semitones, not a recording of a real event); every `pro_*`
category (its refs are trims of launch films, mostly under music or voice, so no clean A/B is possible).
Notes per category for the listener: the impact bench ref is a library cinematic hit (designed by a person from
recordings); riser bench refs are real cymbal swells while the made risers are designed; swish bench refs are the same
files as three whoosh_fast bench refs (each used once).

## Audio measures

- `frames_db(x, ms)`: channel-mean power per whole frame in dB (+1e-20). A file shorter than one frame gives one
  value over the whole file.
- `onset(x, below=40)`: the sample of the first 1 ms frame within 40 dB of the loudest. `end_of(x, below=50)`: the end
  sample of the last 5 ms frame within 50 dB of the loudest. `extent_s(x)` = (end_of - onset) / SR: the audible
  extent. It is used only for picks and the pair class.
- `raised(n)` = 0.5 - 0.5 cos(pi k / max(1, n)) for k = 0 .. n-1.
- `power_frames(x, n, smooth=1)`: channel-summed power, mean per n-sample frame, the last partial frame zero-padded.
  With smooth > 1 and more than 1 frame: a centred moving average of `smooth` frames (same length).
- `loud_max(x, win)`: pad 50 ms of silence on both sides, K-weight (BS.1770, from unit listen features), channel-summed
  square. A signal shorter than the window is zero-padded to it. Return -0.691 + 10 log10(max mean over every
  win-sample window, at every sample position, + 1e-20).
- `meter_m400(x)`: the listen unit's momentary loudness curve (400 ms window, 10 ms hop) of x padded to 400 ms; its max.
  A cross-check.
- `true_peak(x)`: the listen unit's true peak in dBTP (4x oversampled).
- `rms_dbfs(x)` = 10 log10(mean square + 1e-20), -200 for an empty signal. `stereo_corr(x)` = sum(L R) / sqrt(sum L^2
  sum R^2 + 1e-30).

## File facts

- `manifest_entry(p)`: the refset manifest record of a ref file (matched by normalised path), or None. A made final
  has none.
- `source_floor(p)`: the recording floor of a bench ref, measured in its source recording. Read the source from 20 s
  before the trim to 20 s after (inside the file). Power per 50 ms frame. Drop frames at or under 2e-10 (digital
  silence, -100 dB). Floor = the 5th percentile x 10^(gain_db / 10) (the trim's gain). None when the source is not on
  this machine. Cached per path.
- `end_fade_s(p)`: a ref's `fade_out_s` from the manifest (else 0.03); a made final 0.01.
- `analyse(x, cls, fade_s, floor_pow=None, cut_file=True)`: with hop and smooth of ENV[cls]:
  - p = the smoothed power frames; top = their max in dB; e = frame dB - top. e1 = unsmoothed 1 ms frame dB - top.
  - ip = the loudest frame. guard = ceil((fade_s + 5 ms) / hop) frames; stop = max(ip + 1, frames - guard): the
    sound before the file's own end fade.
  - `reach` = the min of e over the last 15 ms before stop (not before ip): where the sound stands as the end fade
    begins.
  - `head` (only for a cut file, a bench ref) = the max of e1 at 2-5 ms (frames 2..4; the whole e1 max when there are 5
    frames or fewer). It shows whether the cut went into the sound just after the ref's own 2 ms fade in. Made finals
    start from digital silence: head None.
  - `peak_late` = the loudest frame starts at or after 85 % of the file (a riser that builds to its end).
  - `strike` = max e1 over the first 15 ms >= -6 (a struck sound).
  - `floor` = floor_pow in dB - top, or None.
- `facts(p)` = (extent, trims cleanly). Analyse on the class of the file's own extent. Clean when all hold: head is
  None, or head <= -30, or a strike; reach <= -30 or peak_late; floor None or <= -50; stereo_corr > -0.5. Cached.

## The trim of a pair

Both sounds of a pair are trimmed the same way. Levels are dB relative to each sound's own loudest envelope frame.

1. Class: `long` when max(extent of real, extent of made) >= 1.0 s, else `short`.
2. Analyse the real clip with its end fade and source floor; the made clip with its end guard, no floor, not a cut file.
3. Pair levels:
   - L_end = max(-50, each reach + 3, each floor + 10), capped at -6. A level both decays reach naturally, above any
     floor.
   - L_on = max(-50, each head + 1, each floor + 10), capped at -6.
4. keep_end = both clips are peak_late.
5. `cut(x, info, L_on, L_end, keep_end)`:
   - on = the start of the first 1 ms frame with e1 >= L_on (0 when none). start = on - 4 ms.
   - j = the last envelope frame before stop with e >= L_end (ip when none). end = min(len, (j + 1) hop).
   - f0 = the end of the last frame up to j with e >= L_end + 6 (or the end of ip). Clamp f0 into [on, end - 5 ms].
   - keep_end holds only when also e[stop - 1] >= L_end. Then end = the file's end, j = the last frame, and f0 = end
     - 2 ms (a safety fade).
   - Clip = x[max(0, start):end]; zero-pad the front when start < 0 (the file has no 4 ms lead). Multiply the first
     4 ms by raised(4 ms). Multiply the last (end - f0) samples by the reversed raised curve.
   - Edge facts: `cut_in_db` = the max e1 over the first half of the fade in (from max(0, start)), -200 when empty
     (what the fade in audibly ramps); `cut_out_db` = the max e over the 5 ms after frame j (the last frame value when
     j is the last frame) (what the trim dropped); `floor_exposed_ms` (with a floor) = the time of envelope frames
     from on to f0 that are under floor + 6 dB (the sound dips to the floor between events).
   - Facts kept: onset_s, start_s, end_s (4 decimals), fade_out_ms, cut_in_db, cut_out_db, reach_db, head_db, floor_db,
     floor_exposed_ms, kept_file_end, end_fade_s, top_db.

## Level match

`match_pair(real, made, cls)`: g = loud_max(real) - loud_max(made) on WIN[cls] (the made gain); gc = TARGET[cls] -
loud_max(real) (the common gain); tp = max(true_peak(real), true_peak(made) + g) + gc; over = max(0, tp - TP_MAX).
Return (g, gc - over, over): when a true peak would pass -1 dBTP, both clips go down together.

## Cleanliness of one pair, as played

Per clip measure: loud_pair_measure (WIN[cls]), loud_other_window (the other WIN), meter_m400, true_peak_dbtp,
sample_peak_dbfs, preroll_dbfs (RMS of the clip before its onset), length_s, stereo_corr.

Pair numbers: the absolute differences of pair measure, other window, meter and true peak; the max true peak; the max
pre-roll level; floor as played = floor_db + top_db + real gain - 10 log10(2) (dBFS per channel); floor margin =
min(L_end, L_on) - floor_db; pre-roll ms = the max over clips of (onset - start); the length ratio; the differences of
cut_in_db and cut_out_db.

FAIL, with these messages in our words: pair difference > 0.5 LU; max true peak > -0.95 dBTP; pre-roll > 5 ms; a
cut_in_db >= L_on (the pre-roll holds sound); floor margin < 6 dB; floor exposed > 100 ms while the floor as played is
above -70 dBFS (else a warning when exposed > 30 ms); onset trims differ by > 10 dB with the louder above -20 dB; the
same for end trims.
WARN: length ratio > 1.6; true-peak difference > 6 dB; other-window difference > 2 LU; floor as played > -60 dBFS;
one clip mono (corr >= 0.995) and the other wide (< 0.9); a clip with anti-phase stereo (corr < 0).

The symmetric row (it never says which side is real, so it is safe to read before listening): class, `matched_on`
(`K-weighted loudest <ms> ms`), the 4 differences, max true peak, max pre-roll level, pre-roll ms, floor as played,
floor margin, floor exposed ms, onset and end levels, the sorted lengths and ratio, the sorted onset and end trim
levels, the sorted fade-out lengths and the sorted stereo correlations.

## Picks

`allocate(n)`: pass 1 gives each PLAN category 1 trial, pass 2 raises each to min(2, its PLAN count), in PLAN order,
while trials remain. More trials than the plan: round-robin over the plan, adding 1 to a category while it has fewer
than min(finals, bench refs). No growth possible: exit `cannot build <n> trials: not enough finals / bench refs`.

`source_of(p)` = the file stem without a trailing `_<digits>` (sister trims share a source).

`pick_trials(n, seed)`:
1. alloc = allocate(n). Bench refs per category. digest = md5 of each bench ref's float32 samples (the same file can
   sit in two categories).
2. Order categories by (distinct clean digests / trials, distinct sources / trials, name): the most constrained picks
   first.
3. `available(c)`: the unused (by digest) bench refs. Pools in order: clean and from an unused source; clean; unused
   source; any. Return the first non-empty pool.
4. Per category: fewer finals than trials exits with an error. gap(m) = min over available refs of |ln(extent(ref) /
   extent(m))|. Pool = finals with gap <= ln 1.3 that trim cleanly; if too few, all with gap <= ln 1.3; if still too
   few, the trials-count finals with the smallest gap (ties by name). Draw the made finals at random from the pool
   without replacement (generator seeded with [seed, 1]), kept in pool order. The pick looks only at length and edge
   facts, never at a score.
5. For each made final: real = the available ref with the closest extent (|ln ratio|, ties by path); exit when none is
   left. Mark the digest and the source used. Record category, sound name, tier, made, real, `source_reused`, the
   pool's basenames, and the clean flags of both.
6. Order: up to 5000 permutations from a generator seeded with [seed, 2]; take the first with no category twice in a
   row (the last one tried otherwise).
7. Sides: half the trials real-as-A, the rest B (an odd count gets one extra A when a coin flip from a generator seeded
   with [seed, 3] says so), then permuted with the same generator. Store `real_is`.

## Build the blind test

`build_blind(out, n, seed, quiet=False, allow_fail=False, dry_run=False)`:
1. Fingerprint (size, modification time) every PLAN final and bench ref.
2. trials = pick_trials(n, seed). Clock t = LEAD.
3. For each trial i (from 1): load both; trim the pair; match the levels; made x 10^((g + gc) / 20), real x
   10^(gc / 20). Measure both clips (pre-roll = the first 4 ms). Run the cleanliness check with the real gain gc.
   Put the real clip on side `real_is`. A starts at round(t, 2) and B at round(A start + A length + GAP, 2): the
   sheet times are exact on a 10 ms grid. Also make `t<NN>_A.wav` and `t<NN>_B.wav` with 0.25 s of silence on each
   side. Next t = round(B start + B length + BETWEEN, 2).
4. Key entry per trial: trial, category, sound, tier, A_start, B_start, lengths of A and B, `real` (A or B), the real
   and made files (repo-relative), source reused, clean-trim flags, the file lengths and audible extents, class, onset
   and end levels, both trim fact sets, the pool the made final was drawn from, a text that names the match method,
   made gain, pair gain, the peak-cap drop, the levels as played, the fails and warnings, the category note, and the
   md5 of both files.
5. Fingerprint again. Any change: exit with `input files changed while building (...)`, nothing written. A polish
   round may be writing finals. The folder keeps its previous complete build.
6. Total length = t - BETWEEN + TAIL. Place the clips into one zero buffer of round(total x SR) + 1 frames. Check that
   every sample outside the clips is exactly 0 (else a FAIL `gaps are not digital silence`) and that no sample is above
   10^(TP_MAX / 20) + 1e-6 (else a FAIL).
7. Summary: status PASS or FAIL, trials, pairs with fails, pairs with warnings, the max of loudest-moment difference,
   meter difference, true peak, true-peak difference, pre-roll level, floor as played, length ratio; the min floor
   margin; gaps silent; all fails and warnings (each prefixed `trial <i> (<sound>):`).
8. `--dry-run`: print a one-line status with the fails and warnings, write nothing, return.
9. Any FAIL without `--allow-fail`: print `CLEANLINESS FAIL (nothing written; ...)` and the fails, exit code 2.
10. Check that the key and the cleanliness record serialise to JSON before anything is replaced. Delete the old
    `t*_[AB].wav`, write the trial files and `blind_test.wav` (48 kHz, PCM 24-bit stereo; every sample must be
    finite).
11. Verify from the written files: read each trial file back, drop the 0.25 s pads, and measure again the pair-measure
    difference and the max true peak. Store them as `verified_from_written_files`. Above the FAIL limits: status FAIL
    with `the written trial files do not hold the matched levels`.
12. Write `cleanliness.json` (indent 1): a short text that says no answers are inside, the FAIL and WARN limits, the
    method in 3 short texts (trim, level, floor), the summary and the symmetric rows.
13. Write `KEY_open_after.json`: seed, built time, trials (the key), excluded categories, the cleanliness summary, a
    `how` text, and `residual_signs_made_vs_real`: for the other loudness window (eps 0.1 LU), the meter (0.1 LU), the
    true peak (0.5 dB) and the length (log ratio, eps ln 1.1), the counts of trials where the made clip is more, less
    or the same.
14. Write `score_answers.py` (below).
15. Write `answer_sheet.txt`: listening instructions in our words (A then B per trial; one is a real recording, the
    other was made entirely in code by the forge; write which one is real, or `?`; do not open the key first; both are
    trimmed alike, play equally loud at their loudest moment, and the real ones come from a held-out set); the scoring
    command with the trial count; the chance line `By chance you would get about <n/2> of <n> right; <k05> or more is
    unlikely by luck (p = <p>)`, where k05 is the smallest k with P(at least k of n by guessing) < 0.05; the cleanliness
    paragraph (status, max loudest-moment difference, max true peak, the fade facts, floors and margin, warnings
    count), wrapped at 112 columns; a table of trial, sound, A start, B start and a blank `____`; and the total length,
    format and counts. Print it unless quiet, with the warnings.

For n = 20, k05 = 15 and p = 0.021 (P(>= 14) = 0.058).

## `score_answers.py` (written into the test folder)

`python score_answers.py A B B A ...`: one letter per trial in trial order (commas allowed), A or B for the side the
listener thinks is real, `?` for undecided. A wrong count or letter exits with a usage message. It reads
`KEY_open_after.json` beside itself. It prints per trial: the trial number, sound, answer, the real side, `right`,
`wrong` or `skipped`, and both file names. Then per sound `<k> of <n> right`; per tier (strong, middle, weak) `<k> of
<n> right (p = ...)`; overall `<k> of <n> right; ... p = ...` with p = the one-sided binomial P(at least k of n) =
sum over i >= k of C(n, i) / 2^n. A `?` is left out of n. It prints that p < 0.05 means the listener can tell made from
real for at least some sounds, and otherwise the test could not show it. When P(at most k of n) < 0.05 it adds that so
few right is itself unlikely: the listener heard the made sounds as real more often than not.

## The progress file

`build_progress(pdir, results_path=None, quiet=False)` (skipped with `--dry-run` or `--only blind`; default folder
`<out>/../progress`):
- V1 sounds (answer name, category, first-version final folder, file stem): key press / typing_key, UI click / click,
  pop / pop, fast whoosh / whoosh_fast, impact / impact. Their finals are `<stem>_01.wav` and `<stem>_02.wav`.
- Results: the first-version results `eval/forge/results_v1.json` and the newest `eval/forge/results_v2r<N>.json`
  (largest N), or `--results`. None found: exit.
- Listening file `v1_vs_v2.wav`: per sound, v1 final 1, its nearest v2 final, v1 final 2, its nearest v2 final (each v2
  final used once). Nearest = the smallest RMS dB distance of the frozen tone profiles + 6 x |log2(extent ratio)|. The
  4 sounds of a group are set to TARGET on the pair measure of the group class (long when any sound is 1 s or more),
  and one common drop per group keeps every true peak at or under -1 dBTP. 1 s between sounds, 3 s between groups
  (2 s added after the 1 s), lead 1 s, tail 1 s. `progress.txt` lists every start, version, loudest level, true peak,
  file and the nearest distance. `progress.json` lists the picks with gains.
- `progress.md`: per V1 sound a table of 13 checks (label, v1 median, v2 median, real loo, real x, verdict), plus the
  inside row (`more` / `fewer` / `same` with a 0.4 step on the medians). A stale warning names categories whose
  finals changed after the v2 results were made (a different file set, or a file modified more than 60 s after the
  results' `created` time). A `KEPT_*.txt` marker in a final folder adds its stem to the heading. A closing table
  counts better / worse model-free checks per sound, and a short "how to read" list in our words: verdicts compare
  against the real rows; model checks saturate and are only up / down; the inside row mixes 4 model and 3 model-free
  checks; first-version impact numbers are optimistic; the blind test decides.
- Checks (key, rule, eps): var_cos span 0.01; tone_db low 0.3; env_oct low 0.03; curve_db low 0.5; t40_ratio one
  0.03; artifacts low 0.5; stereo_corr span 0.02; floor_db span 1.0; darken span 0.05; typ_z, phrase_z, margin,
  as_names model 0.05. For var_cos the real values are the bench and forge diversity cos_mean.
- `verdict(rule, a, b, reals, eps)` (reals = the real loo and real x values, Nones dropped; empty when a or b is None):
  - model: `up` when b > a + eps, `down` when b < a - eps, else `same`; add ` (v1 sat above the real rows)` when a >
    max(reals).
  - low: dist(v) = max(0, v - min(reals)) (min 0 with no reals).
  - one: lo = min(1, reals), hi = max(1, reals); dist(v) = max(0, ln(lo / max(v, 1e-3)), ln(max(v, 1e-3) / hi)).
  - span: no reals gives ''; dist(v) = max(0, min - v, v - max).
  - Both distances 0: `same (both within the real rows)`. Else `better` when dist(b) < dist(a) - eps, `worse` when
    dist(b) > dist(a) + eps, else `same`.

CLI options: `--out`, `--progress`, `--seed` (default 2026), `--n` (20), `--results`, `--only blind|progress`,
`--allow-fail`, `--dry-run`. Relative paths are relative to the repo.
