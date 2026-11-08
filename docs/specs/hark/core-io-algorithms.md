# hark spec: core-io, part: Algorithms and Constants

Unit: **core-io**. This file is part of the core-io spec. The index is `docs/specs/hark/core-io.md`.
The naming rule of the index applies: package, command and module `hark`; environment variables `HARK_*`.

---

## 4. Algorithms

### 4.1 Content ids and cache keys

Purpose: a cache entry must change when the file content changes, even when a same-size file was copied in with its
old timestamp kept (copy with preserve, robocopy, unzip).

Sampled hash of a file of size S:

1. Block size B = 4096 bytes, block count K = 16.
2. Offsets: `floor(i * max(0, S - B) / max(1, K - 1))` for i = 0..15; remove duplicates; sort. The first and last
   blocks are always included.
3. SHA-1 over the bytes read at each offset (up to B bytes each, in offset order). Keep the first 12 hex characters.
4. A read error gives the literal `unreadable`.

File state string: `<absolute path>|<size>|<mtime in ns>|<sampled hash>`.

- `file_id(path)` = first 20 hex of SHA-1(state string).
- Decode key = first 20 hex of SHA-1(`<state>|<sr>|<channels>|v<DECODE_VERSION>`). `DECODE_VERSION` = 2. Bump it
  when decoded samples change.
- `array_id(x)` = first 20 hex of SHA-1(float32 C-contiguous bytes of x + the text of its shape).

hark's caches are its own, so the exact key strings need not match the source. The properties must hold: path, size,
mtime and sampled content all take part; sample rate, channel count and decode version take part in the decode key.

### 4.2 ffmpeg failure reason

1. Split ffmpeg's stderr into lines.
2. From each line, remove a leading `[<component> @ <hex address>]` prefix and trim.
3. Drop empty lines and lines that start (case-insensitive) with `error opening output` or `conversion failed`.
4. Take the last remaining line, else `ffmpeg failed`. Cut to 200 characters.

### 4.3 Quiet replace

`replace_quietly(tmp, dst, tries=5)`: try an atomic rename of tmp over dst. On an OS error, sleep `0.05 * (attempt +
1)` s and retry, up to `tries` attempts. When all fail, delete tmp (ignore errors) and return False. Return True on
success. It never raises. Reason: on Windows the target can be briefly locked by another process. Losing a cache entry is
acceptable; failing the user's command for it is not.

### 4.4 Error line

`error_line(cmd, msg)`: collapse every run of whitespace (newlines included) in msg into one space and trim; an empty
result becomes `unknown error`. Print `hark <cmd>: error: <msg>` to stderr.

### 4.5 JSON cleaning

Before any JSON output, convert recursively:

- dict keys to strings; tuples and lists to lists;
- numpy floating scalars to float; numpy integer scalars to int; numpy arrays to (cleaned) lists;
- float NaN and +/- infinity to null.

### 4.6 CLAP text embedding

1. A single string is a list of one.
2. Key per text: first 24 hex of SHA-1 of its UTF-8 bytes.
3. Texts not in the in-memory cache (deduplicated) are embedded in batches of 64: processor with padding, model text
   features (when the model returns an output object, take its pooled output), cast to float32, divide each row by
   its L2 norm + 1e-12.
4. When anything new was embedded, save the cache: under the process lock, read the cache file again, merge in the
   in-memory entries (other processes may have added entries), write to a temporary file and rename it into place
   with up to 5 tries (as 4.3). When all tries fail, delete the temporary file and keep the in-memory cache.
5. Return the rows in input order, `(n, 512)`. An empty input returns `(0, 512)`.

The cache file is a numpy `.npz` archive: key -> `(512,)` float32. It is read without pickle; a file that fails to read
is an empty cache. The file must be closed after reading (an open archive blocks the rename on Windows).

### 4.7 CLAP audio embedding

Constants: window W = 10.0 s (480,000 samples at 48 kHz), hop H = 5.0 s.

`embed_audio(arrays, pad, batch, per_window)`:

1. A single 1-D array is a list of one. Each array is made mono by the channel mean.
2. Segment each array: when its length is at most W, it is one segment. Else cut windows of W with hop H, with a last
   window that ends exactly at the end (3.4 `windows` with `pad_last=True`).
3. Prepare each segment:
   - shorter than 480 samples (10 ms): zero-pad at the end to 480 samples, so the front end has frames to read;
   - `pad='zero'` and shorter than W: zero-pad at the end to W (better for judging one isolated hit);
   - cut to at most W samples.
   - With `pad='repeatpad'` (default) a short segment goes to the processor as it is; the processor repeat-pads it to
     10 s (CLAP's training convention). Because no segment is ever longer than W, the processor's random-crop path
     never runs, so results are deterministic.
4. Run in batches (default 8): processor with audio and sampling rate 48000, cast floating inputs to the model
   precision, model audio features (pooled output when an object), cast to float32, L2-normalise each row (+1e-12).
5. Group the window embeddings by their source array.
   - `per_window=True`: return a list with one `(k_i, 512)` array per input.
   - Else the mean of each group, L2-normalised again (+1e-12): `(n, 512)`.

`embed_file(path, start, end, pad)`:

1. Key = first 24 hex of SHA-1(`<file_id>|<start>|<end>|<pad>`), where None prints as `None` and floats print as
   Python's shortest repr. File `a_<key>.npy` in the CLAP cache folder.
2. A readable cached file is returned.
3. Else load the mono range (3.2), embed one array, save with a temporary file and `replace_quietly`.

`embed_files(paths, pad, batch)`: the same key with start and end None. Read every cached one; load and embed the
rest in batches of `batch` files; write each result to the cache. Return `(n, 512)` in input order; an empty input
returns `(0, 512)`.

### 4.8 AST class probabilities

Constants: rate 16,000 Hz, window 10.24 s = 163,840 samples, hop half a window = 81,920 samples, minimum segment
1,600 samples (0.1 s).

`probs(arrays, reduce='max', batch=8)`:

1. A single 1-D array is a list of one. Each array: channel mean to mono, then resample 48 kHz to 16 kHz (soxr HQ).
2. Segment: when the length is at most one window, one segment. Else windows of 163,840 samples at starts 0, 81,920,
   ... while a full window fits; when `(len - window)` is not a multiple of the hop, add one window that ends exactly
   at the end.
3. In batches of 8: zero-pad each segment at the end to at least 1,600 samples; run the AST feature extractor at
   16 kHz (it builds the model's fixed-size log-mel input and pads or truncates to the model's frame count; ignore
   its mel-filter warnings); cast floating inputs to the model precision; run the model; apply the logistic sigmoid to
   the float32 logits.
4. Reduce the windows of each input by the per-class maximum (`reduce='max'`) or mean (any other value).
5. Return `(n, 527)` float32. An empty batch list returns `(0, 527)`.

`probs_file(path, start, end, reduce)`: key = first 24 hex of SHA-1(`<file_id>|<start>|<end>|<reduce>`); file
`p_<key>.npy` in the AST cache folder; same read and write rules as 4.7.

`top(p, k, min_p)`: the k highest classes in descending order, keep those with p >= min_p, each as
`(label, p rounded to 3)`. `top_file(path, k, start, end)` = `top(probs_file(path, start, end), k)`.

### 4.9 Hub resolution and fetch

Hub cache paths:

- `slug(id)` replaces every `/` with `--`.
- `model_dir(id)` = `<HF_CACHE>/models--<slug>`.
- `snapshot_dir(id, rev)`: when rev is a 40-hex commit, `<model_dir>/snapshots/<rev>`. Else read the text file
  `<model_dir>/refs/<rev>` (a ref with `/`, such as `refs/pr/2`, is a nested path); when it holds a 40-hex commit,
  that commit's snapshot folder; else None. A missing refs file gives None.

File presence: a name is present when it exists in the folder, following symlinks. A dangling symlink (an interrupted
download) counts as missing. A requirement can be a set of alternatives; any one satisfies it; the first alternative
names it in `missing`.

Weights alternatives:

- safetensors: `model.safetensors`, then `model.safetensors.index.json` (a sharded checkpoint);
- pickle: `pytorch_model.bin`, then `pytorch_model.bin.index.json`. Pickle loads only from a legacy snapshot or from a
  user's local folder.

`is_folder(id)`: true when id is non-empty, id is not one of the pinned ids, and id (with `~` expanded) is an existing
folder. A folder in the working directory that happens to have a pinned id's name never replaces the pin.

`resolve(id, revision=None)` (filesystem only):

1. Local folder: path = absolute, `~` expanded. Required: `config.json` and one weights alternative (safetensors
   first, then pickle). All present: state `cached`, revision `local`, snapshot = path, weights = the first present
   alternative, folder true. Else state `incomplete`, snapshot None, `missing` lists what lacks.
2. Pinned id with revision None or equal to the pin: wanted revision = the pin; required = the pin's files plus its
   weights file; the legacy fallback is allowed only when revision is None.
3. Any other case (unpinned id, or a pinned id with another explicit revision): wanted revision = revision or `main`;
   required = `config.json` plus the safetensors alternatives; not pinned; no legacy.
4. Folder = `snapshot_dir(id, wanted)`. When nothing required is missing: state `cached`, snapshot = folder,
   weights = the present alternative, pinned as in step 2 or 3.
5. Else, when a legacy pin exists and is allowed: when the legacy snapshot holds the pin's files plus the legacy
   weights file, state `legacy`, snapshot = legacy folder, weights = the legacy weights name, `missing` = what the
   wanted revision lacks, pinned true.
6. Else state `incomplete` when the wanted folder exists, else `missing`; snapshot None; weights = first alternative;
   `missing` filled.

`fetch(spec)`:

1. Local folder: raise `RuntimeError`: the label lacks the missing files; hark loads a local folder as it is and
   never downloads into it; put `config.json` and `model.safetensors` (or an older checkpoint's `pytorch_model.bin`)
   there, or set the model variable to a Hub id.
2. An offline flag is set: raise `RuntimeError`: the label is incomplete in the cache (with the missing files) or not
   in the cache (with the cache folder), the flag is set so hark will not download; run once without the flag
   (`hark doctor --fetch` downloads about 1.1 GB), copy a cache there, or point `HARK_HOME` at one.
3. Download with huggingface_hub's snapshot download: repo id, wanted revision, cache folder `HARK_HOME/hf`, and an
   allow-list of file names:
   - pinned: exactly the pin's files plus its safetensors weights (never the pickle);
   - unpinned: the top-level names of 5.5 "custom allow-list". The list is by exact name, never by broad globs,
     because in this matcher a `*` also matches `/`. A wide pattern such as `*.json` then downloads files from
     every sub-folder of the repo, including alternative weight formats, which can be several GB.
4. Any download exception: raise `RuntimeError("could not download <label> into <cache>: <Type>: <message>")`.
5. Resolve again. Not `cached`: raise `RuntimeError` naming what still lacks and the model folder; for an unpinned
   model add that hark loads safetensors weights only.
6. Return the new Spec.

`ensure(id, revision)`: resolve; when a snapshot loads (`local`), return it; else `fetch`.

### 4.10 Embedding cache names

Purpose: each checkpoint gets its own embedding folder. The pin and its legacy snapshot have identical tensors, so
they use one folder.

`cache_tag`:

- pinned: empty string;
- local folder: `--local` + first 8 hex of SHA-1 of `<normalized absolute path>|<weights file size>|<weights mtime
  ns>` (just the path when the weights file cannot be read). New weights in the same folder get a new cache;
- other: `--` + the first 8 characters of the snapshot commit, or of the revision with every character outside
  `[0-9A-Za-z._-]` replaced by `_` when no snapshot loads.

`cache_name`:

- local folder: `local--` + its base name with every character outside `[0-9A-Za-z._-]` replaced by `_`, cut to 40
  characters (`model` when empty), + `cache_tag`. It is one folder name and never a path, because a Windows drive
  letter in a path would put the cache outside the cache root;
- other: `slug(id)` + `cache_tag`. For the default models this is `laion--larger_clap_general` and
  `MIT--ast-finetuned-audioset-10-10-0.4593`.

`hub.cache_name(id, revision)` = `resolve(id, revision).cache_name`.

### 4.11 Thread count

`threads()`: when `HARK_THREADS` is set and non-empty, `max(1, int(value))`. Else `max(1, min(4, cpu // 2))` with
cpu = the OS CPU count, or 2 when unknown. hark never uses a GPU.

### 4.12 Precision

`native_bf16(torch)`, true only when the CPU has native bf16 matrix instructions:

1. When torch reports its CPU kernel capability as one of `DEFAULT`, `NO AVX`, `AVX`, `AVX2`, `ZVECTOR`, `VSX`:
   false (torch's kernel level is below AVX-512, so no bf16 instructions are reachable). A probe error is ignored.
2. When `ONEDNN_MAX_CPU_ISA` (upper-cased) is one of `SSE41`, `AVX`, `AVX2`, `AVX2_VNNI`, `AVX512_CORE`,
   `AVX512_CORE_VNNI`: false.
3. When torch's CPU module reports AVX-512 BF16 support, or AMX tile support: true. A missing probe (old torch) or a
   probe error counts as no.
4. Else false.

Reason: without native support torch still runs bf16 but emulated; the source measured that 16 times slower than
fp32 on an AVX2-only CPU.

Choice:

1. When `HARK_PRECISION` is not `bf16`, or `native_bf16` is false: fp32.
2. Else convert the model to bf16 and run a 2x2 bf16 matrix product in inference mode as a smoke test. On success:
   bf16. On any error: convert the model back to fp32, use fp32.

The source measured bf16 1.4 times faster than fp32 on its development CPU, with embeddings within cosine 0.9993 of
fp32. On Apple Silicon (this fleet's host) the result is fp32.

Inputs: floating input tensors are cast to the chosen precision. Outputs are cast to float32 before numpy.

### 4.13 Features: envelope

All envelope work is on the mono mix `m` = mean of L and R of the stereo decode (not the ffmpeg downmix).

`smooth_env(m, win_ms)` gives one RMS value per millisecond:

1. Hop h = 48 samples (1 ms). Half-window w2 = `max(1, floor(win_ms * 48000 / 2000))` samples (120 for 5 ms, 240
   for 10 ms).
2. Centres c_k = `48 k + 24` for k = 0, 1, ... while `48 k < n`.
3. Window [a, b) = [c_k - w2, c_k + w2] clipped to [0, n].
4. Value = `sqrt(sum of m^2 over [a, b) / max(b - a, 1) + 1e-20)`. Use a running sum of squares (float64).
   Purpose: a bare 1 ms frame RMS drops near each zero crossing of low tones (under about 500 Hz) and is unstable on
   noise; the wider centred window removes both effects.

`frame_rms(m, win)`: non-overlapping frames of `win` samples, the remainder dropped; per frame
`sqrt(mean(x^2) + 1e-20)` in float64. When the input is shorter than one frame, one value over all of it.

`envelope_times(m)`:

1. e5 = `smooth_env(m, 5)`, e10 = `smooth_env(m, 10)`.
2. First event: i0 = first index with `e5 >= 0.1 * max(e5)` (within 20 dB of the loudest). `attack_at = i0 / 1000`.
3. Local rise: L = e5[i0 .. i0 + 299] (up to 300 ms). `attack_ms` = first index j in L with `L[j] >= 0.9 * max(L)`,
   as a float. So an instant onset reads about 4 ms (the 5 ms window), and a sound that starts at full level reads
   about 0 ms. Null when L is empty.
4. Peak: ip = index of the maximum of e10. `peak_at = ip / 1000`.
5. Decay: A = e10[ip ..]. For each i, M_i = max of A[i .. i + 20] (the next 20 ms; past the end, the last value
   repeats). `decay_ms` = the first i with `M_i < e10[ip] * 10^(-30/20)`, as a float. Null when there is none (a
   sustained sound, or one cut off or buried in its own noise floor).

Times are in ms because the envelope has one value per ms.

### 4.14 Features: levels and loudness

Input x is `(n, 2)` float.

- Peak: `20 log10(max |x|)` over both channels (floor 1e-12).
- True peak: oversample x by 4 with polyphase resampling (scipy's default anti-alias filter, a Kaiser window with
  beta 5.0), then the peak in dB.
- RMS: `20 log10(sqrt(mean of x^2 over all samples of both channels))`.

K-weighting (ITU-R BS.1770 at 48 kHz): two biquads in series per channel, coefficients in 5.4 (first the
high-shelf stage, then the high-pass stage). Run in float64.

`loudness_curve(x, win, hop)`:

1. y = K-weighted x. Running sums of y^2 per channel.
2. Block length w = `floor(win * 48000)`, hop h = `floor(hop * 48000)`.
3. When n < w: one block over all samples; its mean square per channel; time = n / 48000 / 2.
4. Else blocks start at 0, h, 2h, ... while `start + w <= n`. Time = (start + w/2) / 48000.
5. Loudness of a block = `-0.691 + 10 log10(sum over channels of mean square + 1e-20)` (channel weights 1).

`integrated_lufs(x)`:

1. Blocks of 0.4 s, hop 0.1 s (75% overlap).
2. Absolute gate: keep blocks with L > -70. None kept: return -70.0.
3. Relative threshold = `10 log10(mean(10^(L/10))) - 10` over the kept blocks.
4. Keep blocks with L > threshold. Result = `10 log10(mean(10^(L/10)))` over them. None kept: threshold + 10.

`lra(x)`:

1. Blocks of 3.0 s, hop 0.1 s. Absolute gate L > -70. Fewer than 5 blocks left: null.
2. Relative threshold = power-mean loudness of the kept blocks - 20.
3. Keep L > threshold. When more than 4 remain: 95th percentile minus 10th percentile (linear interpolation
   percentiles). Else null.

`measure` computes `lufs` from 0.4 s, `short_term_max_lufs` = max of `loudness_curve(x, 3.0, 0.5)` from 3 s, and
`lra_lu` from 6 s.

### 4.15 Features: other time-domain values

- `clipped_samples`: count over all samples of both channels with `|x| >= 0.999`. A mono source duplicated to two
  channels counts each clipped instant twice.
- `dc`: mean of m, 5 dp.
- `noise_floor_dbfs`: `frame_rms(m, 960)` (20 ms); 10th percentile (linear); in dB, 1 dp.
- `dynamic_range_db`: dB of the maximum of the same frames, minus the rounded noise floor, 1 dp.
- `digital_silence_frac`: fraction of m with `|m| < 1e-6`, 4 dp. This is weak evidence of code-made audio on its own:
  real recordings that were cut, gated or denoised also contain exact zeros (the source measured a
  fraction of 59% in one real-recording library).
- Stereo: mid = (L + R) / 2, side = (L - R) / 2. When RMS(side) > 1e-7: `side_to_mid_db = dB(RMS side) - dB(RMS mid +
  1e-12)`, else null. When the standard deviation of both L and R is above 1e-9: `correlation` = Pearson r of L and
  R, else the field is absent.

### 4.16 Features: spectrum

Over the whole analysed range of m, in float64:

1. FFT size: `nfft = 2^e` with `e = ceil(log2(max(1024, min(n, 65536))))` clamped to [10, 16].
2. Welch power spectral density at 48 kHz, segment length `min(nfft, n)`, Hann window, 50% overlap, density scaling,
   mean averaging, constant detrend (the scipy defaults). Add 1e-30 to every bin. Let f be the bin frequencies, P the
   values, T = sum(P).
3. `centroid_hz` = `floor(sum(f P) / T)` (the integer part).
4. `rolloff95_hz`: C = cumulative sum of P / T; index = the first position where C >= 0.95 (a left-side sorted
   search), capped at the last bin; the integer part of that bin's frequency.
5. `bands_db`: for each band [lo, hi) of 5.2, `10 log10(sum of P with lo <= f < hi / T + 1e-12)`, 1 dp. The band
   `top` ends at 24,000 Hz exclusive, so the Nyquist bin is in no band.
6. `bandwidth_hz`: third-octave edges `20 * 2^(k/3)` for k = 0..30 (31 edges, 30 bands, 20 Hz to 20,480 Hz). Band level
   = `10 log10(mean of P in [lo, hi))`, or -300 when the band has no bin. Keep bands with level >= max level - 60.
   Result = the integer part of the upper edge of the highest kept band.
7. `flatness` = `exp(mean(ln P)) / mean(P)` over all bins, 4 dp.
8. `tonal_peak_ratio_db` = `10 log10(max(P) / median(P))`, 1 dp. A pure tone gives a very large value.

`brightness_hz(m)`:

1. A 2-D input is reduced to the channel mean. Under 1024 samples: null.
2. STFT at 48 kHz: segment 1024, overlap 768 (hop 256), Hann window, no boundary extension, zero-padded to whole
   segments (scipy defaults otherwise). A = magnitudes.
3. Frame energy e_t = sum of A over frequency. Active frames: `e_t >= max(e) * 10^(-30/20)`. None active: null.
4. Per active frame, centroid = `sum(f A) / (sum(A) + 1e-20)`. Result = integer part of the median.
   Purpose: this follows perceived brightness; the power centroid of step 3 above is dominated by the low end and is
   kept for compatibility.

### 4.17 Features: tonality, pitch, onsets (not with `--light`)

These use librosa (section 6). y22 = m resampled to 22,050 Hz (soxr HQ). yt = the first 30 s of y22 (enough for
tonality, keeps long files fast).

1. When yt has at least 2048 samples:
   - HPSS: librosa's harmonic-percussive separation with its defaults (STFT 2048 / hop 512, median filters of 31,
     margin 1, soft masks with power 2). `harmonic_ratio = E_h / (E_h + E_p + 1e-20)` with E the sum of squares of
     each part, 3 dp.
   - Pitch: YIN with fmin 50 Hz, fmax 2000 Hz, 22,050 Hz, frame length 2048 (hop 512). Frame RMS with frame 2048, hop
     512, cut to the pitch frame count. Voiced = pitch values of frames with RMS > 0.1 * max RMS. When any are voiced
     and `harmonic_ratio >= 0.8`: `f0_hz` = median, 1 dp. Any error in this step is ignored (no `f0_hz`).
2. Onsets: librosa onset detection on y22 (the whole range, not only 30 s), hop 256, backtracking on, peak-pick delta
   0.07, times in seconds, 3 dp. `onsets` = the first 200; `onset_count` = all; `onsets_per_s` = count / duration,
   2 dp.

### 4.18 Features: tempo, beats, key (`--music`, from 5 s)

1. Onset strength envelope of y22 with hop 256. librosa's beat tracker on it (22,050 Hz, hop 256, times).
   `tempo` = the first tempo value, 1 dp; `beats` = beat times, 3 dp. When more than 4 beats:
   `beat_ibi_std_ms` = 1000 * population std of the beat intervals, 1 dp.
2. Key (Krumhansl-Kessler): constant-Q chroma of m at 22,050 Hz (resampled), hop 512, averaged over time into a
   12-vector c. For each mode (major, minor) and each tonic k = 0..11: r = Pearson correlation of c with the mode
   profile rotated right by k (so the profile's first value lands on pitch class k). Name = `<note[k]> <mode>` with
   the note names of 5.4. Sort all 24 by r descending (ties: by name descending) and return the best 3 as
   `{key, r (3 dp)}`.

---

## 5. Constants

### 5.1 Audio

| Name | Value |
|---|---|
| Analysis rate | 48,000 Hz |
| Audio extensions | `.wav .flac .ogg .oga .opus .mp3 .m4a .aac .aif .aiff .wma` |
| Video extensions | `.mp4 .mov .m4v .webm .mkv .avi` |
| Media extensions | audio + video |
| Lossy codecs (probe) | `aac mp3 mp2 opus vorbis wmav2 ac3 eac3` |
| Decode version | 2 |
| Sampled hash | 16 blocks of 4096 bytes, 12 hex kept |
| Id length | 20 hex (file id, array id, decode key); 24 hex (embedding keys) |
| ffprobe timeout | 60 s |
| ffmpeg reason max length | 200 characters |
| Probe error max length | 200 characters |
| Bit-rate sanity limit | 10,000,000 b/s |
| Replace retries | 5 tries, sleep 0.05, 0.10, 0.15, 0.20, 0.25 s |
| dB floor | 1e-12 (-240 dB) |
| `check_paths` parts listed | 5 |

### 5.2 Feature bands

| Band | Range (Hz, lower inclusive, upper exclusive) |
|---|---|
| sub | 20 - 60 |
| low | 60 - 250 |
| lowmid | 250 - 1,000 |
| mid | 1,000 - 4,000 |
| presence | 4,000 - 8,000 |
| air | 8,000 - 16,000 |
| top | 16,000 - 24,000 |

### 5.3 Feature thresholds and lengths

| Item | Value |
|---|---|
| Too short to measure | under 0.005 s (240 samples) |
| `lufs` needs | at least 0.4 s |
| `short_term_max_lufs` needs | at least 3 s; blocks 3.0 s, hop 0.5 s |
| `lra_lu` needs | at least 6 s; blocks 3.0 s hop 0.1 s; at least 5 gated blocks; more than 4 after the relative gate |
| Momentary blocks | 0.4 s, hop 0.1 s |
| Absolute gate | -70 LUFS (strict greater) |
| Relative gate | -10 LU (integrated), -20 LU (LRA) |
| Loudness offset | -0.691 dB |
| LRA percentiles | 10th and 95th |
| Clip threshold | 0.999 |
| Digital zero threshold | 1e-6 |
| Noise-floor frame | 20 ms (960 samples), 10th percentile |
| Side threshold | RMS 1e-7 |
| Correlation std threshold | 1e-9 |
| Attack envelope | 5 ms RMS, 1 ms hop; event threshold 0.1 (-20 dB); rise to 0.9 within 300 ms |
| Decay envelope | 10 ms RMS; -30 dB; must stay down 20 ms (21-value forward max) |
| Welch FFT size | 1024 .. 65536 (power of 2) |
| Bandwidth bands | third octaves from 20 Hz, 30 bands, 60 dB below the loudest |
| Empty-band level | -300 dB |
| Brightness STFT | 1024 / overlap 768; active within 30 dB (amplitude) of the loudest frame |
| Librosa rate | 22,050 Hz |
| Tonality span | first 30 s; needs 2048 samples |
| YIN | 50 - 2000 Hz, frame 2048 |
| Voiced RMS threshold | 0.1 of max |
| f0 needs | harmonic_ratio >= 0.8 |
| Onset hop, delta | 256, 0.07, backtrack on |
| Onsets listed | 200 |
| Music features need | at least 5 s |
| Chroma hop | 512 |
| Silent summary threshold | peak under -70 dBFS; "digitally silent" at digital_silence_frac >= 0.999 |

### 5.4 Standard tables

K-weighting at 48 kHz (BS.1770), `b` numerator, `a` denominator:

| Stage | b0, b1, b2 | a0, a1, a2 |
|---|---|---|
| 1 (high shelf) | 1.53512485958697, -2.69169618940638, 1.19839281085285 | 1.0, -1.69065929318241, 0.73248077421585 |
| 2 (high pass) | 1.0, -2.0, 1.0 | 1.0, -1.99004745483398, 0.99007225036621 |

Krumhansl-Kessler key profiles (pitch classes C, C#, D, Eb, E, F, F#, G, Ab, A, Bb, B):

| Mode | Values |
|---|---|
| major | 6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88 |
| minor | 6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17 |

Note names in order: `C C# D Eb E F F# G Ab A Bb B`.

### 5.5 Models

| Item | CLAP | AST |
|---|---|---|
| Default Hub id | `laion/larger_clap_general` | `MIT/ast-finetuned-audioset-10-10-0.4593` |
| Pinned revision | `16c8cc3159a3c8a31e8ff5ef1f66b0d9ab3667db` | `f826b80d28226b62986cc218e5cec390b1096902` |
| What the pin is | The Hub's `refs/pr/2`: the safetensors conversion commit (2024-05-06) on top of main `ada0c23` (2023-10-31). Same files as main plus `model.safetensors`. PR refs stay valid after a close or merge. | main (2023-09-06), which already has `model.safetensors`. |
| Pinned weights | `model.safetensors` | `model.safetensors` |
| Pinned other files | `config.json`, `preprocessor_config.json`, `merges.txt`, `special_tokens_map.json`, `tokenizer.json`, `tokenizer_config.json`, `vocab.json` | `config.json`, `preprocessor_config.json` |
| Legacy snapshot (source only) | revision `ada0c23a36c4e8582805bb38fec3905903f18b41`, weights `pytorch_model.bin`, same other files | none |
| Input rate | 48,000 Hz | 16,000 Hz |
| Window | 10.0 s, hop 5.0 s | 10.24 s, hop 5.12 s |
| Minimum input | 480 samples | 1,600 samples |
| Output | 512-d unit vector | 527 sigmoid probabilities |
| Hearing range | 50 Hz - 14 kHz | up to 8 kHz |
| Default batch | text 64, audio 8 | 8 |
| Fallback logit scale | 100.0 | - |

Custom allow-list for an unpinned Hub model (exact names; the last uses `?` single-character wildcards):
`config.json`, `preprocessor_config.json`, `feature_extractor_config.json`, `processor_config.json`,
`tokenizer.json`, `tokenizer_config.json`, `special_tokens_map.json`, `added_tokens.json`, `vocab.json`, `vocab.txt`,
`merges.txt`, `tokenizer.model`, `spiece.model`, `sentencepiece.bpe.model`, `model.safetensors`,
`model.safetensors.index.json`, `model-?????-of-?????.safetensors`.

Sizes: about 1.1 GB for both pinned models; the CLAP safetensors file is about 776 MB.

Load-time sanity values (doctor `--load`): first load about 20 - 40 s for both; one window is a 1 s zero signal.

Precision sets (4.12): capability names that mean no bf16: `DEFAULT`, `NO AVX`, `AVX`, `AVX2`, `ZVECTOR`, `VSX`;
oneDNN caps that mean no bf16: `SSE41`, `AVX`, `AVX2`, `AVX2_VNNI`, `AVX512_CORE`, `AVX512_CORE_VNNI`.

Offline flag true values: `1`, `TRUE`, `YES`, `ON` (trimmed, any case).

### 5.6 Doctor data files

`vocab.json`, `vocab_stats.json`, `realness_model.json`, `launch_profile.json`, all in the package data folder.
They belong to units listen (first three) and timeline (last one).

### 5.7 Doctor package list

Import name -> distribution name:

| Import | Distribution | Required for a healthy result |
|---|---|---|
| numpy | numpy | yes |
| scipy | scipy | yes |
| soundfile | soundfile | yes |
| soxr | soxr | yes |
| librosa | librosa | yes (Q4) |
| PIL | pillow | yes |
| matplotlib | matplotlib | yes |
| torch | torch | ML tier (Q2) |
| transformers | transformers | ML tier (Q2) |
| mcp | mcp | no (optional) |

The source also lists `pyloudnorm`, `sklearn` (scikit-learn) and `requests`; hark drops them unless another unit
needs them (Q4).
