# hark spec: core-io, part: Behaviour

Unit: **core-io**. This file is part of the core-io spec. The index is `docs/specs/hark/core-io.md`.
The naming rule of the index applies: package, command and module `hark`; environment variables `HARK_*`.

---

## 3. Behaviour

### 3.1 Cache root

1. When `HARK_HOME` is set, the root is its absolute path.
2. Else, when the folder that contains the package folder holds a `pyproject.toml` and is writable, the root is
   `<that folder>/.cache`. This keeps a source checkout's caches inside the checkout.
3. Else the root is `~/.cache/hark`.

The root is computed once at import. Sub-folders: `hf`, `emb`, `decoded`. `ensure_dirs()` creates all four folders
(root and the three) when missing. Every code path that writes a cache calls it first.

### 3.2 Audio loading

Purpose: every module reads audio through one function. All analysis runs at 48 kHz float32.

1. Input: any file ffmpeg can read: audio files and the audio track of a video. `load` does not check the extension.
2. If the path does not exist: raise `AudioError("no such file: <path>")`.
3. Channel count: 2 by default (stereo), 1 when `mono=True`.
4. Cache lookup (when `cache=True`): compute the decode key (4.1). If `<DEC_CACHE>/<key>.npy` exists and reads,
   use it. A cache file that fails to read is ignored and the file is decoded again.
5. Decode (when not cached), through ffmpeg:
   - Decode to raw 32-bit float little-endian PCM on stdout, no video, at the requested rate and channel count.
     ffmpeg does the resampling with its default resampler. ffmpeg runs quiet (errors only) and never reads stdin.
   - Stereo request for a mono source: ask ffmpeg for 1 channel and duplicate it to both channels at unity gain.
     Reason: ffmpeg upmixes mono to stereo at -3 dB per channel, which would read 3 dB low on every level meter.
     The source channel count comes from ffprobe on the first audio stream (0 when ffprobe fails).
   - Mono request for a stereo source: use ffmpeg's own downmix (`-ac 1`). Do not compute it in numpy. Reason: this
     is the model input convention, and the model calibrations of other units were measured with it. The source
     states that this downmix sums a correlated stereo signal up to 3 dB hotter than the mean of L and R. Level
     readings therefore use the stereo decode (features does this).
   - Result shape: `(n, 2)` for stereo, `(n,)` for mono.
6. Decode failure (non-zero ffmpeg exit, or empty output): probe the file (3.3), then raise `AudioError` with the
   first matching message:
   - probe worked, the file has a picture track and no audio codec: `<path> has no audio track (a silent render? mux
     the soundtrack in first)` (wording may differ; it must name the path and say there is no audio track).
   - ffmpeg exited 0 with no output, or its stderr says the input does not contain any stream: `<path>: no audio
     stream`.
   - otherwise: `cannot decode <path>: <reason>`, where reason is the last meaningful ffmpeg stderr line (4.2).
   Every message is one line.
7. Cache write (when `cache=True` and the decode was fresh): save to a temporary file next to the target, named with
   the process id, then move it into place with `replace_quietly` (4.3). When the cache write fails, the decoded audio is still returned.
8. Finite check (on cached and fresh data alike): when any sample is NaN or infinite, raise `AudioError`. The message
   gives the number of bad sample frames (a frame is bad when any channel is bad), the time of the first one in
   seconds with 3 decimals, and the two likely causes (a corrupt file, or code that divided by zero). Required shape:
   `<path>: <N> NaN / Inf sample(s), first at <T> s: ...`.
9. Range: when `start` or `end` is given, cut after the decode. First sample `max(0, round(start * sr))` (start None
   is 0). End sample `min(n, round(end * sr))`, or n when end is None. The cache always holds the whole file.
   `load` itself does not reject odd ranges; it can return an empty array. Callers validate ranges (features does).

`to_mono(x)`: mean over channels for 2-D input; 1-D input is returned as is. This is a plain mean, not the ffmpeg
downmix.

`resample(x, a, b)`: returns `x` unchanged when the rates are equal; else the soxr resampler at quality `HQ`,
output float32.

`duration(path)`: length of the mono decode divided by 48000.

### 3.3 Probe

`probe(path)` runs ffprobe (60 s timeout) for: per stream codec type, codec name, sample rate, channels, bit rate,
width, height, frame rate, and the attached-picture disposition; and the container duration and bit rate.
Output as JSON.

Result:

| Field | Rule |
|---|---|
| `codec` | Codec name of the first audio stream, or null. |
| `src_rate` | Its sample rate as int, 0 when absent. |
| `channels` | Its channel count as int, 0 when absent. |
| `kbps` | The audio stream bit rate when it is all digits and under 10,000,000 b/s. Else the container bit rate, but only when the file has no picture track. Divided by 1000 and rounded to int. Null when neither applies. Reason: some Vorbis files report 2^32 - 1. |
| `lossy` | True when the codec is in the lossy set (5.1). |
| `duration` | Container duration in s, 3 dp, 0 when absent. |
| `has_video` | True when there is a video stream that is not an attached picture (cover art does not count). |
| `video` | Only when `has_video`: `{width, height, fps}`; fps is the frame-rate fraction evaluated, 3 dp, null when its denominator is 0. |

When running ffprobe or parsing its output raises (not found, timeout, bad JSON): return
`{"probe_error": "<message cut to 200 characters>"}` and nothing else. When ffprobe runs but prints nothing, the
result has the fields with empty values (codec null, rates 0, has_video false).

### 3.4 Path expansion

`list_media(paths, recursive=True, exts=MEDIA_EXTS)`:

1. A single string is a list of one.
2. A folder: walk it (sub-folders too, unless `recursive=False`), folders and files in sorted name order, keep files
   whose lower-case extension is in `exts`.
3. An existing file: keep it whatever its extension.
4. Anything else: treat it as a glob pattern (`**` matches across folders). Keep matches that are files with a media
   extension, sorted. Folders that match are dropped.
5. Remove duplicates by normalized absolute path (case-folded on case-insensitive systems), keep the first.

`check_paths(paths, what='paths')`: for every given path that yields no media file, build a part
`<path> (no such file or folder)` when it does not exist, else `<path> (no audio or video files in it)`.
When any part exists, raise `AudioError("<what>: nothing playable in <part>, <part> ...")` with at most 5 parts, then
` ...` when more. Purpose: when one input of a batch is misspelled, the user is told, instead of the batch running without it.

`windows(x, win, hop, sr, pad_last)`: window and hop in samples are `round(win * sr)` and `round(hop * sr)`. When
the input is not longer than one window, the result is one window `(0.0, x)`. Else windows start at 0, hop, 2 hop,
... while a full window fits. With `pad_last=True` and a remainder, add one last window that ends exactly at the end
(start `n - w`). Each item is `(start_seconds, segment)`.

`db(v, floor=1e-12)`: `20 log10(max(v, floor))`. The floor gives -240 dB for silence.

`file_id(path)` and `array_id(x)`: stable content ids used as cache keys (4.1).

### 3.5 CLI shell

Purpose: one fast command with one output convention.

1. Thread hold for BLAS. Before any numeric library is imported:
   - When `OPENBLAS_NUM_THREADS` is set, or numpy is already imported, do nothing.
   - Else scan the arguments for `--threads N` and `--threads=N`; the last one wins. Convert to int. When it is
     missing or not above 0, use `config.threads()`. When it is not an integer, do nothing (the parser reports it).
   - Set `OPENBLAS_NUM_THREADS` to that number.
   Reason: the OpenBLAS copies inside numpy and scipy size their thread pools to the logical CPU count when they load.
   On Windows each thread also reserves memory, measured by the source at roughly 0.4 GB per process on a 12-CPU
   machine. The heavy work runs in torch, so the extra BLAS threads give no speed.
2. Find the command: the first argument that is not an option. Improvement for hark: also skip the value that
   follows a global `--threads`. (The source takes the first token that does not start with `-`, so
   `--threads 2 measure` picks `2`, falls back to loading every command module, and still works, only slower.)
3. Fast start: when the command is known, import only that command's module and register only its parser. When that
   import fails, print `hark: the "<command>" command is unavailable: <ExceptionType>: <message>` on stderr and exit
   2.
4. Otherwise (help, no command, unknown command): import every command module in table order. A module that fails to
   import is skipped with a stderr note `(hark: module <module> unavailable: <ExceptionType>: <message>)`; the other
   commands stay available. Two commands that share a module register once (rank and find; compare and ab).
5. Parse the arguments. Parser errors exit 2 with the parser's usage message.
6. When `--threads` is given with a non-zero value (either position), set `HARK_THREADS` to it for this process.
   Negative values pass through and `config.threads()` makes them 1.
7. No command (or a command parser without a handler): print the help and exit 0.
8. For every command except `mcp`: make stdout replace unencodable characters with backslash escapes, so printing a file
   name that the console encoding cannot represent does not raise. `mcp` keeps stdout untouched because it is the protocol
   channel.
9. Run the handler. Its return value is the exit code; None is 0.
10. When the handler raises: with `HARK_DEBUG` set, re-raise (traceback, exit 1). Else print one error line
    `hark <command>: error: <ExceptionType>: <message>` (4.4) and exit 1.

Expected-error path of the listen commands (`describe`, `match`, `rank`, `find`, `compare`, `ab`): their handler
catches `AudioError`, `ValueError` and `OSError` itself. It prints the error line without the exception type
(`hark <command>: error: <message>`). With `--json` it also prints `{"summary": "error: <message>", "error":
"<message>"}` on stdout so a JSON-parsing caller gets an object. Exit 1. This wrapper is part of this unit's report
helpers and is used by unit listen. All other errors of those commands use step 10.

### 3.6 Output convention (report)

Every command builds a result dict with a plain-language `summary` and details. Conventions for values: times in
seconds with 3 decimals; levels in dBFS, LUFS or dBTP with 1 decimal; cosine similarities with 3 decimals; calibrated
scores as z (standard deviations above a background) or as probabilities 0..1.

`emit(result, args, text=None)`:

1. When `--out FILE` is given: make its parent folders, write the JSON (4.5) in UTF-8 with non-ASCII characters
   kept as they are, indent 1.
2. When `--json` is given: print the JSON with indent 1 and every non-ASCII character escaped (`\uXXXX`), so the
   output parses as JSON whatever encoding the console uses (for example cp1252 on a redirected Windows stdout). Then a newline.
3. Else print the text form: the `text` argument when given; else the result's `text` field when present; else the
   generic rendering. When the console cannot encode a character, print it as a backslash escape instead of failing.
4. Return the result.

Generic text rendering: the summary on the first line, then one `key: value` line per other field in insertion
order, skipping `summary` and `text`. Lists and dicts print as compact JSON; when that is longer than 600 characters,
cut it to 600 and add ` ...`.

`fmt_t(t)`: seconds to `m:ss.ss` (minutes without padding, seconds zero-padded to 5 characters with 2 decimals).
None gives `-`. Example: 63.5 gives `1:03.50`; 4.19 gives `0:04.19`.

### 3.7 Offline and download policy

1. `config.offline()` reads the three flags at call time, in order `HARK_OFFLINE`, `HF_HUB_OFFLINE`,
   `TRANSFORMERS_OFFLINE`. A flag is set when its value, trimmed and upper-cased, is `1`, `TRUE`, `YES` or `ON`.
   Return the first set flag's name, else None.
2. hark never downloads while a flag is set. A model that is not cached is then an error that names the flag.
3. hark never sets or clears any of the three flags. (A test scans the code for that.)
4. hark loads a model only from a complete local folder. It never passes a repo id to the model library, so a cached
   load never contacts the Hub, and a damaged cache is an error that names the folder, never a silent new download.
5. A missing model downloads once, at its pinned revision, safetensors weights only (except the pickle rules of 4.9),
   into `<HARK_HOME>/hf`.
6. A local model folder (named by `HARK_CLAP` or `HARK_AST`) loads as it is, with no network and no revision. hark
   never downloads into it.

### 3.8 Model loading

Common to both models:

1. Create the cache folders.
2. Model id: the argument, else the configured id. Revision: the configured override, but only when the id is the
   configured one; else None (the pin, or `main`).
3. `hub.ensure(id, revision)` gives the Spec to load (4.9). This is where a first download happens.
4. Import torch and the transformers classes only now. Set torch's thread count to `config.threads()`.
5. Load the model and its processor from `spec.snapshot` with local files only. The model load asks for
   safetensors unless the Spec's weights file is a pickle (`pytorch_model.bin` or its index). Pickle weights load with
   torch's safe `weights_only` loader (the default of torch 2.6 and later, which is the floor).
6. Quiet the library: transformers log level error, no progress bars; ignore warnings about mel filters and
   `FutureWarning` during the load.
7. A load failure raises `RuntimeError` with: the Spec label, the class that failed, the exception type and message.
   - For hark's cache, add the snapshot folder, say the files can be damaged or the transformers version cannot
     read them, and say that deleting the folder makes hark fetch the pinned files again, or to run `hark doctor`.
   - For a user's local folder, say the files can be damaged or unreadable by this transformers version. Never
     suggest deleting the user's folder.
8. Put the model in eval mode. Choose precision (4.12).
9. Embedding cache folder: `<EMB_CACHE>/<clap|ast>/<cache_name>--<precision>`. Create it.
10. Each model is a process-wide singleton behind a lock: `get()` returns the loaded instance, and loads a new one
    only when none exists or a different `model_id` is passed. The MCP server keeps it warm between calls.

CLAP specifics:

- Model class: CLAP model plus CLAP processor (transformers).
- `logit_scale`: exponent of the model's audio logit scale parameter; 100.0 when it cannot be read.
- Text embeddings (4.6) and audio embeddings (4.7), 512-dimensional unit vectors.
- The CLAP mel front end covers 50 Hz to 14 kHz at 48 kHz input. It cannot hear content above 14 kHz. Use the
  features (bandwidth, bands) for low-pass checks.

AST specifics:

- Model class: AST audio classification model plus AST feature extractor (transformers).
- `labels`: the model's id-to-label map in index order (527 AudioSet classes); `index` maps label to position.
- Input is resampled to 16 kHz, so it hears up to 8 kHz only.
- Its probabilities are independent per class (sigmoid), trained supervised on AudioSet, so they are calibrated per
  class (CLAP cosines are not).

Load time: the first load of both models in a fresh process takes about 20 to 40 s on a laptop CPU. Every CLI call
is a new process and pays it, except `measure`, `sfx`, `kit` and the other model-free paths of section 6. For many
calls, use the MCP server or one Python process with `hark.api`.

Download size: about 1.1 GB for both pinned models. The pinned CLAP safetensors file alone is about 776 MB.

### 3.9 Doctor

Purpose: one command that says whether hark can run here, and what is missing.

`hark doctor [--load] [--fetch]`. Without `--load` it imports nothing heavy and takes a few seconds.

Steps of `check(load, fetch_missing)`:

1. Basics: Python version, cache root, thread count, precision setting, configured model ids.
2. Packages: for each package of 5.7, check that it is importable without importing it (find its import spec), then
   read its installed version from package metadata. Found without metadata: `ok`. Not found:
   `MISSING (<ExceptionType>)`. Reason: importing torch alone takes 10 to 20 s.
3. Tools: path of `ffmpeg` and `ffprobe` on PATH, else a `MISSING` string; the ffmpeg one says to install ffmpeg and
   put it on PATH.
4. `torch_cuda_used`: false. `offline`: the set flag or null.
5. With `--fetch`: for each model, resolve it. Already cached: `already cached` (a local folder: `a local folder
   (nothing to download)`). Else fetch it: `downloaded <first 7 of the commit>`, or `failed: <message>` on a
   `RuntimeError`. A model on a legacy snapshot is not `cached`, so `--fetch` downloads its pinned safetensors.
   Fetch never raises.
6. Models: per model `{revision, cached (the state), loads (sha), weights, snapshot, missing (list)}`, filesystem
   only; `models_cached`: whether a snapshot loads.
7. agy (hark-specific): the path of `agy` on PATH, or null. Missing agy is a note, not a problem, because the LLM
   critique is optional. (The source instead reports whether its LLM API key variables are present. hark never
   reads API keys here.)
8. Data files: each file of 5.6 exists in the package data folder.
9. Sound bank: summary of the bank as loaded (unit bank-kit-sfx): `version`, `sounds` (entry count), `types` (count
   per entry type, an entry without type counts as `forge`), `pluck` (entries whose id starts with `pluck/`), `files`
   (one row per bank file: `{file, entries}`; the file is its base name when it lives in hark's own bank folder, else
   the full path), `extra` (the non-empty parts of `HARK_BANK`). Any exception becomes `{error: "<Type>: <msg>"}` and
   the check continues.
10. Problems (each a string):
    - every package that is `MISSING`, except the optional ones (`mcp`, and the LLM client package when one exists);
      see Q2 for torch and transformers;
    - `sound bank: <error>` when the bank failed;
    - `ffmpeg missing` (a missing ffprobe alone is not listed as a problem in the source; hark should list it, see
      section 8, E-D3);
    - `data file <package>/data/<name> missing` for each missing data file;
    - `<model>: <failed message>` for each failed fetch;
    - for a local-folder model that is incomplete (and did not just fail a fetch): `<model>: the local model folder
      lacks <files joined by " and "> (hark never downloads into a folder)`;
    - when models are not cached (not local folders, not failed) and an offline flag is set: one problem naming the
      models, the cache folder and the flag, and saying to run `hark doctor --fetch` once without the flag, or to
      copy a cache there.
11. Notes:
    - when models are not cached and no offline flag is set: they are not cached yet; the first call that needs them
      downloads the pinned safetensors files (about 1.1 GB for both) into the cache folder; or run
      `hark doctor --fetch` now;
    - for each model in state `legacy`: it loads the pickle weights of an old cache (short commit and weights file),
      from the cache only; `hark doctor --fetch` downloads the pinned safetensors, used from then on.
12. With `--load`, unless an offline flag is set and a model is not cached (a problem already explains it): load
    CLAP, time the load (1 dp), time one embedding of 1 s of silence (2 dp), record precision, revision and weights;
    the same for AST (load time, one window, revision, weights); record `native_bf16`. A `RuntimeError` becomes the
    problem `model load failed: <message>`. hark addition: a missing torch or transformers also becomes a problem
    (`model load failed: ...` naming the install command of section 6), not a crash (source behaviour: an import
    error escapes and the command exits 1).
13. Summary: one line made of
    - the bank line (when the bank loaded): `Sound bank <version>: <N> sounds (<forge minus pluck> forge, <pluck>
      plucks, <fit> fitted from CC0 recordings[; extra files from HARK_BANK: <list>]).`
    - then `hark looks healthy.` when there are no problems, else `Problems: <p1>; <p2>.` (trailing periods of each
      problem removed);
    - then the notes.
14. Exit code 0 in every case where `check` returns. Problems do not change the exit code.

### 3.10 `hark measure`

CLI:

1. Check the paths (3.4, `what` = `files`). Expand them to a media list. An empty list is an `AudioError` naming the
   inputs.
2. For each file: `measure(file, music, start, end, light)`, add `file` and `summary`.
3. One file: emit its result. Several: emit `{summary: "<N> files measured.", files: [...]}`. The text form is the
   per-file summary lines joined by newlines (the "N files measured." line is not printed in text form).
4. Exit 0. Errors (bad range, missing file, decode error) go through the shell's step 10: one error line with the
   exception type, exit 1, no JSON on stdout.

`measure(src, ...)` input:

- A path: decoded stereo (3.2) with the range.
- An array at 48 kHz, `(n,)` or `(n, 2)`. A 1-D array is duplicated to two channels. For arrays, `start` and `end`
  are validated but not applied (source behaviour; see Q5).

Range validation (before any decode), each a `ValueError`:

- `start < 0`: `start must be >= 0 s (got <start>)`.
- `end` given and `end <= start` (start None counts as 0): `end (<end> s) must be greater than start (<start> s)`.
- A path, `start > 0`, the probed duration is known and `start >= duration`: `start <start> s is at or beyond the end
  of <base name> (<duration> s long)`.
- Numbers in these messages use the shortest general format (`5`, `0.5`).

Summary line (text form). Must be one line. Structure, with `<name>` the base name of the file:

- When `error` is set: `<name>: <error>`.
- When `peak_dbfs < -70`: `<name>: <duration 3 dp> s, <what> (peak <peak, 0 dp> dBFS): nothing to measure; ...`
  followed by a short clause that says loudness, spectrum and envelope values of silence mean nothing. `<what>` is
  `digitally silent` when `digital_silence_frac >= 0.999`, else `near-silent`.
- Otherwise, parts joined by `; ` and ended with `.`:
  1. `<name>: <duration 3 dp> s, peak <peak_dbfs> dBFS (true peak <true_peak_dbtp> dBTP)`
  2. when `lufs` exists: `<lufs> LUFS`, plus `, LRA <lra_lu> LU` when that exists
  3. `noise floor <x> dBFS, ` + envelope + `, brightness <b> Hz, bandwidth <w> Hz`, where envelope is
     `attack <a> ms` (when attack exists) then `decay <d> ms`, or, when decay is null, the words
     `no -30 dB decay (sustained or cut off)`; numbers in shortest general format; separated by `, `
  4. when `clipped_samples > 0`: `<n> clipped samples`
  5. when `tempo` exists: `tempo <t> BPM, key <best key or ?>`
  6. when `bands_db` exists: `bands sub <v> low <v> lowmid <v> mid <v> presence <v> air <v> top <v> dB`

Example (illustrative values): `hit.wav: 0.560 s, peak -0.1 dBFS (true peak 0.2 dBTP); -14.2 LUFS; noise floor
-62.0 dBFS, attack 3 ms, decay 190 ms, brightness 1500 Hz, bandwidth 16127 Hz; bands sub -30.1 low -12.0 lowmid
-6.2 mid -3.1 presence -9.8 air -18.0 top -40.2 dB.`

The MCP server has its own measure summary wording (unit surfaces).

### 3.11 Packaging

| Item | Value |
|---|---|
| Distribution and import name | `hark` |
| Python | 3.10 or later |
| Console script | `hark` -> the CLI main function |
| Base dependencies (source) | numpy >= 1.26, scipy >= 1.11, soundfile >= 0.12, soxr >= 0.3, librosa >= 0.10, pyloudnorm >= 0.1, scikit-learn >= 1.3, pillow >= 10, matplotlib >= 3.8, torch >= 2.6, transformers >= 4.40 |
| Extras (source) | `mcp` -> mcp >= 2; `llm` -> requests >= 2.31; `dev` -> pytest >= 8 |
| Package data | `data/*.json`, `data/*.npz`, the bank, kit and seed JSON folders of the forge |
| torch wheel | CPU-only wheel index `https://download.pytorch.org/whl/cpu` |
| External tools | `ffmpeg` and `ffprobe` on PATH |

hark changes the dependency split (section 6): torch and transformers become optional. pyloudnorm and
scikit-learn are declared by the source but no runtime module of the package imports them (only eval and skill
scripts do). hark drops them from the base set unless another unit needs them (Q4).

Install facts (source, for the hark skill to adapt): install with a tool installer from the repository with the
`mcp` extra, then `doctor --fetch` to download the models up front. A development install: create a venv, install
torch from the CPU index first, then the package editable with all extras, then `doctor --load`. In hark the
architect ticket owns the venv (`~/.local/state/hark/venv`) and the launcher `skills/hark/scripts/hark`.

Old-cache facts: a cache made by old source versions holds CLAP at its main revision, whose only weights are a
pickle. It still loads from the cache (never downloaded), doctor says so, and `doctor --fetch` downloads the pinned
safetensors. hark has no old caches, so hark may drop the legacy rule (Q1).
