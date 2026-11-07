# forge-core part 4: artifact detectors and polish measures

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.polish`: model-free detectors of defects
(clicks, aliasing, DC, harsh resonance, comb ring, cut tails, digital silence, clipping), polish measures (attack,
onset brightness, tail smoothness, decay, body), loudness of a set of variants, and the penalty rules that turn them
into critic terms.

Purpose. A made sound effect is judged in an edit next to professional library sounds. A click from a hard gate,
foldover on a sweep, a 3 kHz ring, a tail cut at -30 dB or a frozen spectrum is heard at once. The model checks often
miss these faults: their training separates real from synthetic sound, not clean from faulty sound. Each
detector is a separate function that returns plain data.

All detectors use the mono sum (channel mean) unless stated. Input is 48 kHz. A path is loaded with the core-io
loader.

## Interface

| function | returns (keys) |
|---|---|
| `clicks(x, thr=18.0)` | `clicks` (int), `click_times` (first 8, s, 3 decimals), `edge_db` (1 decimal) |
| `aliasing(x, nfft=4096, hop=1024, frames=64, legacy=False)` | `fold` (3 decimals), `harmonic_frames` (int), `nyq_db` (1 decimal) |
| `dc(x)` | `dc_db`, `infra_db` (1 decimal each) |
| `resonances(x, top=3)` | `res_db` (1 decimal), `res_hz` (int or None), `res_peaks` (list of (Hz, prominence dB)) |
| `evolution(x, G=None)` | `evolution_db` (2 decimals or None), `centroid_oct` (2 decimals or None) |
| `ring(x, legacy=False)` | `ring_z`, `ring_ms`, and when not judged `ring_raw_z`, `ring_src` (`'narrow'` or `'pitch'`) |
| `truncated(x)` | `trunc_db`, `end_db` |
| `silence(x)` | `silence_ms`, `silence_max_ms` (1 decimal) |
| `clipping(x)` | `clip_runs`, `overs` (ints) |
| `attack(x)` | `attack_ms` (2 decimals or None), `onset_hf_db` (1 decimal or None) |
| `tail(x)` | `bump_db` (1 decimal), `decay_db_s` (1 decimal or None), `t40_ms` (float or None) |
| `body(x)` | `crest_db` (1 decimal) |
| `lufs_max(x)` | float LUFS |
| `loudness_spread(xs)` | `lufs` (list, 2 decimals), `spread_lu`, `std_lu` (2 decimals; None for an empty list) |
| `measure(x, G=None)` | the union of every detector's keys above (G = a precomputed bandgram for evolution) |
| class `PolishRef(refs=(), designed=False, gated=False, targets=None)` | see below |

Module constants: `SR = 48000`; `K_SOS` (the ITU-R BS.1770 K-weighting filter at 48 kHz as 2 second-order
sections, Constants); `ALIAS_PEAK_DB = 10.0`; `ALIAS_FAMILY = 0.5`; `ALIAS_LOUDEST = 4.0` (a power ratio, about
6 dB); `RING_SPAN_HZ = 2000.0`; `RING_PITCH_Z = 100.0`; `RING_SMOOTH_DB = 2.0`; `RULES`; `POLISH`; `HARD_AT = 0.5`;
`GATE_DB = {'designed': -30.0, 'gated': -12.0}`.

`PolishRef` attributes: `M` (measure dicts of the refs), `n`, `designed`, `gated`, `targets` (`{polish key: (lo, hi)}`).
Methods: `check(x, exclude=None, G=None, m=None)` (m = a precomputed measure dict), `words(c, keep=None)`.

Running the module as a program with files or folders prints, per audio file, the basename and the values
`clicks edge_db fold nyq_db dc_db res_db res_hz ring_z trunc_db silence_ms clip_runs evolution_db attack_ms
onset_hf_db bump_db t40_ms crest_db` as `key=value`.

Helpers used below:
- frame power in dB: for consecutive n-sample frames (floor(len / n) of them), 10 log10(mean square + 1e-24); a signal
  shorter than one frame gives one value of -240.
- power STFT: zero-pad to nfft when shorter; frames at hop; Hann window; |rfft|^2; shape (frames, bins).

## Algorithms: artifacts

### `clicks`

Finds isolated discontinuities inside the sound.
1. peak = max |m| + 1e-12. `edge_db` = 20 log10(max(|m[0]|, |m[-1]|) / peak + 1e-12). A file whose first or last sample
   is far from zero gives a click on playback.
2. d2 = the 2nd difference of m (it rises 12 dB per octave, so a jump stands far above smooth content). e = frame power
   dB of d2 in 1 ms frames (48 samples). lvl = frame power dB of m in 1 ms frames. k = min of the two lengths.
3. Only when k >= 24: pad e by 12 frames at each end (edge values). For frame i, the window e[i-12 .. i+12]; pre =
   median of window positions 2..9 (frames i-10 .. i-3); post = median of positions 15..22 (frames i+3 .. i+10).
4. Impulsiveness: per frame, the share of the d2 energy in its 2 largest squared samples.
5. A frame hits when all hold: e - max(pre, post) > thr (18 dB); |post - pre| < 8 dB (the neighbourhoods agree);
   lvl > max(lvl) - 70; the frame is more than 3 frames from the loudest level frame (the sound's own attack);
   impulsiveness > 0.6 (a jump, not a physical tick).
6. Merge runs of consecutive hit frames. Each run is one click at time ((i + j + 1) / 2) / 1000 s, 3 decimals.

### `aliasing` (fold and nyq_db)

Measures foldover of naive oscillators in clearly harmonic frames. Noise, modal and inharmonic sounds have no
harmonic grid and are not judged.

1. S = power STFT (4096, hop 1024). `nyq_db` = 10 log10(energy at >= 20 kHz / total + 1e-12).
2. Frame level dB; active frames have level > max - 30 dB; keep the first `frames` (64) of them.
3. Pw = S summed over bin offsets -2..+2 (energy within +-2 bins; circular shift).
4. Candidate f0 grid c0: 150 Hz x 2^(j / 96) for j from 0 while under 3000 Hz (96 per octave). Harmonic numbers
   H = 1 .. int(4000 / 150). Grid frequencies c0 x H, rounded to bins; `low` marks grid points in [60, 4000) Hz.
5. Per active frame, a harmonic fit:
   - tl = energy in [60, 4000) Hz. None when tl <= 0.
   - Per candidate: the share of tl that lies within +-2 bins of its grid points under 4 kHz, minus 0.02 x its number
     of grid points (a dense grid covers more by chance). Best candidate i. None when its share is under 0.7.
   - Refine: for each harmonic h up to 4000 / c0[i], find the strongest bin within +-3 bins of h c0 / df, refine its
     frequency by a parabola on the log powers of the 3 bins around it, weight by its power. f0 = sum(w h f) /
     sum(w h^2) (weighted least squares through the origin). Return (f0, share).
6. For each frame with an f0 r:
   - Sweep rate = the largest |f0 change| to the previous or next frame's f0, per second (hop / SR apart). smear (bins
     per harmonic) = 0.5 x rate x (nfft / SR) / df.
   - Harmonics hs from ceil(4000 / r) to int(20000 / r); half-widths hw = 2 + hs x smear bins. ok when 2 hw df <
     0.5 r (the window stays within half the spacing). Skip the frame when no harmonic is ok or the first one is not.
     Keep the ok harmonics up to the first gap in the sequence.
   - top = min(20000, (last kept harmonic + 0.5) r). Band [4 kHz, top] in bins b0..b1. th = band energy. Skip the
     frame when b1 - b0 < 20 or th < 1e-3 x frame energy.
   - Legacy mode (the round-2 measure): count all band energy that lies off the harmonic windows: add (1 - min(1,
     on-grid / th)) th to the numerator and th to the denominator.
   - Default mode:
     a. Partials above Nyquist: ka = int(SR / 2 / r) + 1 .. int(4 SR / r). Their fold positions fa =
        |((ka r + SR/2) mod SR) - SR/2|. Alias half-width aw = 1 + ka smear. Keep those with fa in [4000, top].
     b. Nearest harmonic hn = max(1, round(fa / r)) with half-width hwn = 2 + hn smear; gap = |fa - hn r| / df bins.
        Judge only positions with gap >= hwn + 1 (the alias centre is off the harmonic window). The first fold
        (ka < SR / r) gives the strongest aliases. Skip the frame when fewer than 4 judged first-fold positions exist.
     c. Cap aw at max(0.5, gap - hwn - 0.5) so it never reaches into the harmonic window.
     d. Local floor fl = a running median of the band power over 65 bins (about 750 Hz).
     e. Reference level href = the largest power within the windows of the kept harmonics whose frequency is in the
        top half of the band (>= 0.5 top).
     f. For each judged position j: w = the power in its window; hp = the peak power of the neighbouring harmonic's
        window; Hann leakage from that harmonic at distance d = max(2.5, gap - hwn + 2) bins is hp x 10^((-31.5 -
        18 log2(d / 2.5)) / 10) (Hann side lobes: -31.5 dB at 2.5 bins, -18 dB per octave beyond). Threshold =
        10^(ALIAS_PEAK_DB / 10) x max(floor at the window centre, leakage). A hit when threshold < max w <=
        ALIAS_LOUDEST x href (an alias of partial k is quieter than the real partials below it). For first-fold
        positions count nf and hits. For every hit add max(0, sum w - floor x window length) to an excess.
     g. Numerator += min(th, excess) x min(1, (hits / max(1, nf)) / ALIAS_FAMILY) (at least half of the first-fold
        positions must show a peak: a family, not one partial); denominator += th.
   - Count the frame as harmonic.
7. fold = numerator / denominator when the denominator > 0 and at least 2 frames were harmonic, else 0.

Measured calibration: with the default mode, a noise layer under a harmonic tone and inharmonic bell partials over a
chance grid measure 0-0.08 (the legacy measure gave 0.4-0.99 for them), while a naive 440 Hz sawtooth keeps 0.057 and
an FM sound rendered without oversampling keeps 0.53 (real foldover).

### `dc`

`dc_db` = 20 log10(|mean m| / peak + 1e-12). `infra_db` = 10 log10(energy under 15 Hz / total + 1e-12), from the power
spectrum of m zero-padded to the next fast real FFT length. A one-sided puff, an offset or drift shows here.

### `resonances`

The harshest narrow peak.
1. nfft = 4096 when len >= 4096, else the power of 2 at or above the length, at least 1024. Energy spectrum = the
   power STFT (hop nfft / 4) summed over frames. L = 10 log10(S + 1e-30).
2. A log grid g of 48 points per octave from 100 Hz while under 16 kHz. Each grid cell spans [g / 2^(1/96),
   g x 2^(1/96)); Lg = the max L in the cell (at least 1 bin).
3. prominence = Lg - running median of Lg over 33 points (2/3 octave, edge mode nearest).
4. Ear weight: 1.0 for 2-6 kHz, 0.7 for 1-9 kHz outside that, 0.5 elsewhere.
5. Candidates: Lg > max Lg - 30 and prominence > 0 and a local maximum of prominence (>= both neighbours; not the end
   points). With none: `res_db` 0.0, `res_hz` None, empty peaks.
6. Rank by prominence x weight. `res_db` = the best product; `res_hz` = its grid frequency (integer); `res_peaks` =
   the top 3 as (Hz, prominence).

### `evolution`

How much the spectral shape changes, independent of level.
1. G = bandgram (part 3) or the given one. P = 10^(G / 10).
2. 8 octave groups of bands by centre (Hz, [lo, hi)): [0, 112), [112, 224), [224, 447), [447, 894), [894, 1789),
   [1789, 3578), [3578, 7155), [7155, inf). Q = group sums + 1e-12, smoothed over 3 frames (30 ms, "same" mode).
3. tot = row sums. Active rows: 10 log10(tot) > 10 log10(max tot) - 30. With fewer than 8 active rows: both outputs
   None.
4. R = max(10 log10(Q / tot), -40) (each frame's shape against its own total, so a fade alone does not change R).
5. lag = 5 frames (50 ms). Rows i that are active, i >= lag, and whose row i - lag is active. With fewer than 3: None.
6. Group weights sh = the mean of the two rows' shares Q / tot. Change ch = sqrt(sum(sh (R_i - R_{i-lag})^2) /
   sum sh) (quiet groups' estimation noise does not count). `evolution_db` = sum(ch tot_i) / sum(tot_i), energy
   weighted.
7. Centroid per active row: log2(sum(P CEN) / sum P). `centroid_oct` = its 90th minus 10th percentile.

### `ring`

Comb-like metallic ringing in the tail.
1. e = frame power dB in 10 ms frames. Peak frame ip. Start s = ip + 3 (30 ms after the peak). End = the first frame
   from s that is more than 50 dB under the peak, else the last frame. When end - s < 6 frames: ring_z and ring_ms
   None.
2. Tail segment = samples of frames s .. end, at most 16384 samples.
3. Cepstral peak of a segment (helper): N = 8192 when the segment is at most 8192 samples, else 16384. Log power
   spectrum of the Hann-windowed segment zero-padded to N. Keep 300-12 000 Hz. Clamp to at most 60 dB under its
   maximum. Remove a trend: a running mean over 2 kHz worth of bins (at least 3). Cepstrum = |rfft| of the
   Hann-windowed residual. Quefrency axis q = index / (count x df) seconds. Keep 2-40 ms; with fewer than 8 values,
   None. Robust z of the largest value: (max - median) / (1.4826 MAD + 1e-12); return (z, quefrency ms) and, when
   asked, the bandwidth in Hz where the 2 kHz-smoothed spectrum (untrimmed) lies within 40 dB of its maximum.
4. Get (z, ms, bandwidth) of the tail. None result: ring_z None.
5. Output `ring_z` (1 decimal), `ring_ms` (2 decimals). Return it as is in legacy mode or when z < 8.
6. Not judged, `ring_src 'narrow'`: bandwidth < RING_SPAN_HZ. (In a sub drop, a few low partials over an otherwise
   empty band gave z of 120-390 at the 2 ms edge.)
7. Pitch test: the tail's teeth roughness (below) <= RING_SMOOTH_DB means deterministic partials. Else, take the body
   from 2 frames before to 6 frames after the peak; when it has at least 1920 samples, get its cepstral peak (zb,
   msb); when zb >= RING_PITCH_Z and ms / msb is within 30 % of 1, or within 10 % of 2, 3, 1/2 or 1/3, it is the
   sound's own pitch.
8. Pitch: ring_z None, `ring_src 'pitch'`, `ring_raw_z` = z. Else the output of step 5.

Teeth roughness of a segment at delay ms: f0 = 1000 / ms. Log power spectrum with N = 16384. For each multiple k f0
from ceil(300 / f0) to 12000 / f0: the max level within -2..+2 bins of its bin. Keep levels within 50 dB of the
spectrum's max. With fewer than 8, infinity. Else the median |2nd difference| of the levels. The partials of an
oscillator lie on a smooth curve (0.1-1.4 dB on buzzy hums); the teeth of a comb filter driven by noise vary by
6-7 dB.

### `truncated`

A tail cut while still decaying.
1. e = frame power dB in 5 ms frames. Fewer than 12 frames: both None.
2. Peak frame ip; e relative to the peak. end = mean of the last 2 frames.
3. When fewer than 12 frames follow the peak, or end < -55: trunc_db None, end_db = end.
4. before = frames from max(ip, len - 38) to len - 8 (150 ms to 40 ms before the end). slope = a least-squares line
   over them in dB per second (time step 5 ms), when at least 4 frames, else 0.
5. fade_from = mean of frames len-10 .. len-8. expect = max(3, -slope x 0.04) (the natural drop over the last 40 ms).
6. Cut level: when fade_from - end > 3 expect + 6 and fade_from > -50 (the level falls at the end far faster than the decay
   before it): trunc_db = fade_from. Else when slope < -20 dB/s and end > -50 (still decaying when the file stops): trunc_db =
   end. Else None.
7. Return trunc_db (1 decimal or None) and end_db (1 decimal).

### `silence`

peak = max |m| + 1e-12. Audible samples have |m| > 1e-3 peak. With fewer than 2: 0, 0. Between the first and last
audible sample, find runs of |m| < 1e-6. Runs of at least 96 samples (2 ms) count. `silence_ms` = total run length /
48; `silence_max_ms` = the longest / 48. A recording always has a floor; exact zeros inside are an edit or a gate.

### `clipping`

Per channel with a peak > 0: `overs` += the count of |x| > 1.0. Flat flags: for each sample after the first,
|x| >= 0.99 peak and |difference to the previous| < 2e-6. A run of at least 2 flags (3 equal samples) counts 1 in
`clip_runs`.

## Algorithms: polish

### `attack`

1. Hop 12 samples (0.25 ms). n = floor(len / 12); with n < 4, both None.
2. Power per hop; envelope = sqrt of a 2-hop moving average ("same" mode) (0.5 ms RMS).
3. i90 = the first hop with envelope >= 0.9 max. i10 = the last hop at or before i90 with envelope < 0.1 max, else 0.
4. `attack_ms` = (i90 - i10) x 0.25.
5. Onset sample = 12 i10. seg = the next 480 samples (10 ms). When seg has at least 32 samples: power spectrum of the
   Hann-windowed seg zero-padded to 1024; `onset_hf_db` = 10 log10(energy at >= 6 kHz / total + 1e-12).

### `tail`

1. e = frame power dB in 10 ms frames. Fewer than 4 frames: bump 0.0, decay None, t40 None.
2. Smooth the power over 3 frames; back to dB. Relative to the peak frame. after = from the peak on.
3. `t40_ms` = 10 x the first index in after under -40 dB, else 10 x len(after).
4. Running minimum of after. Live frames: after > -45, except the first 2. `bump_db` = max of (after - running min)
   over live frames, else 0. A smooth tail falls monotonically.
5. `decay_db_s`: a least-squares line through the frames with -45 <= after <= -6 (time 10 ms per frame), when at
   least 3, in dB per second.

### `body`

K-weight m (K_SOS, causal). The loudest 50 ms (2400 samples) mean square: the maximum of the moving mean, or for a
shorter signal the total sum divided by 2400. `crest_db` = 20 log10(peak + 1e-12) - 10 log10(that + 1e-24). High means
thin and spiky, with little body at the same peak.

### Loudness

`lufs_max(x)`: per channel, K-weight and square; sum over channels; the loudest 400 ms (19 200 samples) mean, or for a
shorter sound the sum / 19 200 (a zero-padded meter); LUFS = -0.691 + 10 log10(value + 1e-24). This is the momentary
maximum loudness.

`loudness_spread(xs)`: lufs_max per item (paths are loaded); `spread_lu` = max - min; `std_lu` = population standard
deviation.

## Penalty rules (`PolishRef.check`)

### Artifact rules `RULES`

Columns: key, bad direction (+1: high is bad; -1: low is bad), absolute limit, margin, scale, kind, cap.

| key | bad | abs | margin | scale | kind | cap | note |
|---|---|---|---|---|---|---|---|
| clicks | +1 | 0.0 | 0.0 | 1.0 | hard | 2.0 | |
| edge_db | +1 | -40.0 | 6.0 | 10.0 | hard | 2.0 | |
| fold | +1 | 0.02 | 0.02 | 0.03 | hard | 2.0 | |
| nyq_db | +1 | -35.0 | 6.0 | 8.0 | soft | 1.0 | inaudible itself, a sign of naive synthesis |
| dc_db | +1 | -40.0 | 6.0 | 10.0 | hard | 2.0 | |
| infra_db | +1 | -20.0 | 6.0 | 8.0 | soft | 1.5 | wasted headroom, thumps on real systems |
| res_db | +1 | 15.0 | 3.0 | 5.0 | soft | 2.0 | |
| ring_z | +1 | 12.0 | 3.0 | 6.0 | soft | 2.0 | |
| trunc_db | +1 | -45.0 | 6.0 | 10.0 | hard | 2.0 | |
| silence_ms | +1 | 2.0 | 2.0 | 10.0 | hard | 2.0 | |
| clip_runs | +1 | 0.0 | 1.0 | 4.0 | hard | 2.0 | |
| evolution_db | -1 | none | 0.35 | none | soft | 2.0 | frozen: below 0.35 x the refs' median |

For each rule with a value in the measure dict (None values are skipped):
- rv = the kept refs' non-None values of that key (kept = all refs except `exclude`).
- Bad = -1 (evolution): skip unless at least 2 refs and median(rv) >= 1.5 (the refs move). limit = 0.35 x median(rv);
  penalty = min(cap, max(0, 2 (limit - v) / max(limit, 1e-6))).
- Bad = +1: limit = max(abs, max(rv) + margin) when rv is not empty, else abs (when all real recordings have a trait,
  it is not a fault). penalty = min(cap, max(0, (v - limit) / scale)).
- Designed mode, keys clicks and silence_ms: kind becomes soft and the penalty halves.
- Designed mode, key trunc_db: limit = max(limit, GATE_DB['gated'] when gated else GATE_DB['designed']); recompute the
  penalty; when gated, kind becomes soft. A gate that closes after the level has already dropped is intended design.
- Record limits[key] (2 decimals). A positive penalty is recorded (3 decimals). A hard rule at penalty >= HARD_AT
  (0.5) adds the key to `hard`.
- art_pen = sum of the artifact penalties.

### Polish rules `POLISH`

Columns: key, bad direction, kind, margin, scale. These are relative to the refs only.

| key | bad | kind | margin | scale | meaning |
|---|---|---|---|---|---|
| attack_ms | +1 | log | 1.5 | 1.0 | slower than 1.5 x the refs' slowest + 1 ms |
| onset_hf_db | -1 | db | 4.0 | 6.0 | duller snap than the refs' dullest - 4 dB |
| bump_db | +1 | db | 2.0 | 6.0 | bumpier tail than the refs' bumpiest + 2 dB (absolute floor 4) |
| t40_ms | -1 | log | 0.6 | 1.0 | tail shorter than 0.6 x the refs' shortest |
| crest_db | +1 | db | 2.0 | 4.0 | thinner than the refs' thinnest + 2 dB |

For each rule with a non-None value:
- rv = the kept refs' values. When the key is not bump_db, fewer than 2 refs carry it, and `targets` has the key:
  rv = the target (lo, hi) (the critic's absolute study targets).
- bump_db: limit = max(4, max(rv) + margin) when rv is not empty, else 4; p = (v - limit) / scale.
- Other keys with fewer than 2 values in rv: skipped.
- log kind: bad +1: limit = margin x max(rv) + 1; p = log2(max(v, 1e-3) / limit) / scale. Bad -1: limit = margin x
  min(rv); p = log2(limit / max(v, 1e-3)) / scale.
- db kind: mg = max(margin, 0.25 (max(rv) - min(rv))) (3-6 diverse refs pin a range down loosely). Bad +1: limit =
  max(rv) + mg, p = (v - limit) / scale. Bad -1: limit = min(rv) - mg, p = (limit - v) / scale.
- Record limits[key]. A positive p is recorded as min(2, p) (3 decimals).
- polish_pen = sum of the polish penalties.

### Check result

`{'measures', 'limits', 'art' (key -> penalty), 'polish' (key -> penalty), 'hard' (list), 'art_pen', 'polish_pen',
'words'}`.

### Words

Artifact items in order of falling penalty, then polish items in order of falling penalty. The text of each item
(ref ranges come from the kept refs' values, formatted `(refs <min>-<max>)`, with no range when no ref has the key):

| key | text |
|---|---|
| clicks | `<n> click(s) (<first 3 times as 0.000 s>)` |
| edge_db | `starts / ends off zero (<edge_db> dB re peak)` |
| fold | `aliasing: <fold as a percent> of the moving partials fold back` |
| nyq_db | `energy above 20 kHz <v> dB` + range |
| dc_db | `DC offset <v> dB re peak` |
| infra_db | `infrasound below 15 Hz <v> dB` + range |
| res_db | `narrow resonance at <res_hz> Hz +<res_db> dB` + range |
| ring_z | `metallic comb ring every <ring_ms:.1f> ms (z <z>)` + range |
| trunc_db | `tail cut at <v> dB` |
| silence_ms | `<v> ms of digital silence inside` |
| clip_runs | `clipped (<n> flat tops)` |
| evolution_db | `frozen spectrum (<v:.1f> dB of change, refs median <median:.1f>)` |
| attack_ms | `attack <v> ms` + range with 1 decimal |
| onset_hf_db | `dull onset (>6 kHz share <v> dB` + range + `)` |
| bump_db | `bumpy tail (+<v> dB re-rise)` |
| t40_ms | `tail dies fast (t40 <v> ms` + range + `)` |
| crest_db | `thin body (crest <v:.1f> dB` + range with 1 decimal + `)` |

Join as `artifacts: <items>` and `polish: <items>` with `; `. With no item: `clean (no artifact or polish flag)`.

## Constants

K-weighting second-order sections at 48 kHz (b0, b1, b2, a0, a1, a2), from ITU-R BS.1770:
- stage 1 (high shelf): 1.53512485958697, -2.69169618940638, 1.19839281085285, 1.0, -1.69065929318241,
  0.73248077421585
- stage 2 (high-pass): 1.0, -2.0, 1.0, 1.0, -1.99004745483398, 0.99007225036621

These are public standard values.

Detector numbers (also in the steps): click frame 1 ms, threshold 18 dB, neighbourhood agreement 8 dB, level window
70 dB, attack exclusion +-3 ms, impulsiveness 60 % in 2 samples; aliasing STFT 4096 / 1024, 64 frames within 30 dB,
f0 150-3000 Hz at 96 per octave, grid share 0.7 with a 0.02 per-point cost, +-2-bin windows, 4-20 kHz band, local
floor 65 bins, peak 10 dB, family 50 %, loudest 4x, Hann side lobe -31.5 dB at 2.5 bins and -18 dB per octave;
infra 15 Hz; resonance grid 48 per octave 100 Hz - 16 kHz, median 33 points, 30 dB window, ear weights 1 / 0.7 / 0.5;
evolution 8 groups, 30 ms smoothing, 30 dB activity, -40 dB floor, 50 ms lag, 8 rows minimum; ring start 30 ms,
end -50 dB, 6 frames minimum, quefrency 2-40 ms, z gate 8, MAD factor 1.4826; truncation 5 ms frames, end -55 dB,
slope window 150-40 ms, fade test 3 x expect + 6 dB with fade_from > -50, decay test slope < -20 dB/s with end > -50;
silence 1e-6 for at least 2 ms, audibility 1e-3; clipping 0.99 peak and 2e-6 steps; attack 0.25 ms hops, 10 %-90 %,
onset window 10 ms, HF band 6 kHz; tail 10 ms frames, -40 dB t40, -45 dB live, fit range -6..-45 dB; body 50 ms;
loudness 400 ms, offset -0.691.

## Errors and edge cases

- Very short sounds: clicks needs 24 ms; ring needs 6 frames of tail; attack needs 1 ms; tail needs 40 ms; truncated
  needs 60 ms and 60 ms after the peak; evolution needs 8 active frames. Below these, the detector returns its
  neutral or None values, and a None value is never penalised.
- A silent channel is skipped by clipping.
- `PolishRef([])` uses the absolute limits only. Relative polish rules are skipped (bump_db keeps its absolute 4 dB
  floor) unless `targets` supplies a range.
- Evolution is judged only when refs move (median >= 1.5 dB).

## Acceptance tests

P1. A 0.5 s 1 kHz sine with one sample changed by +0.5 at 0.25 s: `clicks` = 1, click time 0.25 within 2 ms. The
    same sine without the jump: 0 clicks.
P2. A sine that ends at full amplitude: `edge_db` > -6; with a 10 ms fade-out: `edge_db` < -60.
P3. A naive (non-band-limited) 440 Hz sawtooth of 1 s: `fold` > 0.02 and at least 2 harmonic frames. A PolyBLEP
    sawtooth (part 2 `osc`): `fold` <= 0.02. White noise: `harmonic_frames` 0 and fold 0.
P4. A signal with a constant offset of 0.1 on a 0.5 peak: `dc_db` about -14 dB; penalty > 0 and `dc_db` in `hard`.
P5. Pink noise plus a sine at 3 kHz 25 dB over the local spectrum: `res_hz` within 2 % of 3000 and `res_db` over 15.
P6. A noise burst with an exponential decay that is cut at -20 dB: `trunc_db` within 3 dB of -20; the same burst
    decayed to -70 dB: None.
P7. A sound with 10 ms of exact zeros in its middle: `silence_ms` = 10.0 and silence_ms in `hard`; with designed=True
    it is soft with half the penalty.
P8. A sine clipped at 0.8 (flat tops): `clip_runs` > 0.
P9. An impulse followed by a decay: `attack_ms` under 1; a 50 ms linear fade-in: `attack_ms` about 40 (10 % to 90 %).
P10. White noise of 2 s with steady level: `evolution_db` under 1.5; a noise swept by a band-pass from 200 Hz to
     8 kHz over 1 s: `evolution_db` over 3 and `centroid_oct` over 3.
P11. `lufs_max` of a 1 kHz full-scale sine of 1 s, mono, is -3.0 LUFS within 0.1 (the BS.1770 reference value).
     The same sine on both channels of a stereo file is 0.0 LUFS within 0.1 (2 channels add 3 dB).
P12. `loudness_spread([x, 0.5 x])` gives spread_lu 6.02 within 0.01.
P13. `PolishRef([r1, r2, r3])` where every ref has a 4 dB tail bump: a sound with a 5 dB bump has no bump penalty
     (limit max(4, 4 + 2) = 6). A sound with a 9 dB bump has penalty (9 - 6) / 6 = 0.5.
P14. `words` of a clean sound is `clean (no artifact or polish flag)`.
P15. A comb-filtered noise tail (noise fed to a feedback comb at 5 ms delay, decaying over 0.5 s): `ring_z` >= 12 and
     `ring_ms` within 0.1 of 5.0. A 200 Hz buzz (pitch period 5 ms) with the same decay: ring_z None with ring_src
     'pitch'.
