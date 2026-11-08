# hark spec: core-io, part: Errors, edge cases and acceptance tests

Unit: **core-io**. This file is part of the core-io spec. The index is `docs/specs/hark/core-io.md`.
The naming rule of the index applies: package, command and module `hark`; environment variables `HARK_*`.

---

## 8. Errors and edge cases

Audio:

- E-A1: Missing file -> `AudioError("no such file: <path>")`, before any ffmpeg call.
- E-A2: A text file renamed to `.wav` -> `AudioError` that starts `cannot decode <path>:` and is one line.
- E-A3: A video with no audio track (a silent render) -> `AudioError` naming the path and saying it has no audio
  track and to mux the soundtrack in first.
- E-A4: ffmpeg not on PATH -> the subprocess start raises an OS error (file not found). In the CLI this is an error
  line, exit 1. Doctor names it as `ffmpeg missing`. hark should give a clear message: `ffmpeg not found on PATH`
  (improvement; the source gives the raw OS error).
- E-A5: ffprobe not on PATH during a stereo decode -> the channel probe returns 0 and the decode continues with
  ffmpeg's own upmix (mono sources then read 3 dB low). The source does not warn.
- E-A6: One NaN at 0.5 s of a 1 s float WAV -> `AudioError` with `1 NaN / Inf sample(s), first at 0.500 s`. The
  check also runs on a cached decode.
- E-A7: A corrupt cache `.npy` -> ignored, the file is decoded again and the cache rewritten.
- E-A8: A cache folder that is read-only or locked -> the decode still returns; the temporary file is removed.
- E-A9: A same-size replacement with the old mtime -> a new file id (sampled hash differs), so no stale cache.
- E-A10: `start` beyond the end in `load` -> an empty array (no error at this level).
- E-A11: Files under 4096 bytes -> every offset is 0; the hash covers the whole file once.
- E-A12: A file that cannot be opened for the sampled hash -> the hash part is `unreadable`; ids still form.
- E-A13: ffprobe with cover art (MP3 or M4A with an attached picture) -> `has_video` false.
- E-A14: A Vorbis stream that reports bit rate 4294967295 -> `kbps` from the container instead.
- E-A15: `list_media` on a glob that matches only folders -> empty list; `check_paths` names the pattern.
- E-A16: The same file twice in a batch (once relative, once absolute) -> listed once.

Features:

- E-F1: Under 5 ms -> `{duration, error: "too short to measure"}`; the summary is `<name>: too short to measure`.
- E-F2: Digital silence -> every value computes (peak -240, loudness -70, decay may be null); the summary says
  digitally silent and nothing to measure.
- E-F3: A steady tone from sample 0 -> `attack_ms` about 0 to 2, `decay_ms` null.
- E-F4: A clip that ends at full level -> `decay_ms` null; summary says no -30 dB decay.
- E-F5: Dual-mono stereo (L equal R) -> `side_to_mid_db` null; `correlation` 1.0.
- E-F6: One silent channel -> `correlation` absent.
- E-F7: `start < 0`, `end <= start`, `start >= duration` -> the `ValueError`s of 3.10, before decoding.
- E-F8: `start = 0` is not checked against the duration (zero is treated as no start).
- E-F9: Array input with `start` or `end` -> validated, not applied (Q5).
- E-F10: A pitch-tracker failure -> no `f0_hz`, no error.
- E-F11: Over 200 onsets -> `onsets` holds 200, `onset_count` holds all.
- E-F12: `--music` under 5 s -> no tempo, beats or key, no error.
- E-F13: A NaN in an intermediate value (for example a constant signal's flatness) -> the JSON writer turns it to null.

Models:

- E-M1: Model not cached, offline flag set -> `RuntimeError` that names the model, the cache folder and the flag;
  nothing downloads.
- E-M2: Model cached, offline flag set -> it loads; no network.
- E-M3: A snapshot with a dangling symlink for the weights -> state `incomplete`; `ensure` downloads again (when not
  offline).
- E-M4: Damaged weights in the cache -> load error that names the folder and says delete it to refetch; no silent
  download.
- E-M5: A local folder without weights -> `incomplete`; `ensure` raises the never-download-into-a-folder error, also
  without an offline flag.
- E-M6: A local folder that fails to load -> error without any delete advice.
- E-M7: A folder named like a pinned id in the working directory -> ignored; the Hub pin is used.
- E-M8: A revision override that is a branch name not in the cache -> `missing`; downloaded with the custom allow-list;
  embeddings cached under `--<first 8 of the commit>`.
- E-M9: A download that leaves no safetensors (a pickle-only repo) -> `RuntimeError` saying hark loads safetensors only.
- E-M10: Network failure during download -> one `RuntimeError` line: `could not download <label> into <cache>:
  <Type>: <msg>`.
- E-M11: Two processes write the text-embedding cache at once -> each merges the other's entries before the rename;
  no entry is lost except under a rename race, which only costs a recompute.
- E-M12: Audio input under 10 ms to CLAP -> zero-padded to 480 samples; still one embedding.
- E-M13: Audio of exactly 10.0 s -> one window; 10.0 s + 1 sample -> two windows (0 s and flush with the end).
- E-M14: bf16 smoke test fails -> fp32, with no error.

CLI:

- E-C1: `hark` alone -> help, exit 0.
- E-C2: `hark nosuch` -> parser error (invalid choice), exit 2.
- E-C3: A command module that fails to import, requested directly -> stderr names the command and the exception, exit 2.
- E-C4: A broken module during `hark -h` -> stderr note for that module; help lists the others; exit 0.
- E-C5: `--threads abc` -> parser error, exit 2; the BLAS hold does nothing.
- E-C6: `--threads 0` -> default threads; `HARK_THREADS` not changed.
- E-C7: A non-ASCII file name with `--json` on a cp1252 console -> valid JSON with `\u` escapes.
- E-C8: A non-ASCII file name in text mode on an ASCII console -> backslash escapes; no crash.
- E-C9: `--out` into a folder that does not exist -> the folder is made.
- E-C10: An uncaught exception with `HARK_DEBUG=1` -> Python traceback, exit 1.
- E-C11: A multi-line exception message -> collapsed to one line.
- E-C12: `--threads` given before and after the command with different values -> the later one sets `HARK_THREADS`
  and the BLAS limit.

Doctor:

- E-D1: Doctor with problems -> exit 0; `problems` non-empty; summary starts with the bank line, then `Problems:`.
- E-D2: A broken bank file -> `bank.error` set, a problem added, other checks still run.
- E-D3: ffprobe missing alone -> the source lists no problem. hark adds the problem `ffprobe missing` (improvement),
  because probe and channel detection need it.
- E-D4: `--load` while offline with an uncached model -> no load attempt; the offline problem explains it.
- E-D5: `--load` with a model that fails to load -> `model load failed: <message>` problem, exit 0.
- E-D6: `--fetch` while offline -> each uncached model reports `failed: <offline message>`, which is also a problem.
- E-D7: `--load` without torch -> hark reports a problem with the install command (the source crashes with exit 1).

---

## 9. Acceptance tests

Fixtures: write test WAVs with soundfile as 32-bit float at 48 kHz unless stated. Tests that need models are marked
ML and are skipped when the ML tier is absent. No test downloads; model tests use a pre-filled cache or fakes.

| Id | Input | Expected properties of the output |
|---|---|---|
| AT-1 | Mono 2 s tone, 0.8 amplitude, f in {40, 80, 200, 1000} Hz, with envelope exp(-t / 0.5), after 0.1 s of silence; `measure(x, light=True)` | `decay_ms` within 2% of 0.5 * ln(10^1.5) * 1000 = 1727 ms; `attack_ms <= 6`. |
| AT-2 | White noise (seed 0), 2 s, 0.5 amplitude, envelope exp(-t / 0.1) | `decay_ms` within 5% of 345 ms. |
| AT-3 | 0.5 amplitude 440 Hz sine, 2 s, starting at sample 0 | `attack_ms <= 2`; `decay_ms` is null. |
| AT-4 | A 1 s file; `measure(path, start=5.0)`; `measure(path, start=0.5, end=0.2)`; `measure(path, start=-1)` | `ValueError` with `beyond the end`; `ValueError` with `greater than start`; `ValueError` with `>= 0`. |
| AT-5 | Write a file, take `file_id`; rewrite it with other samples of the same byte size; restore the old mtime | Size and mtime equal; `file_id` differs. |
| AT-6 | A text file named `corrupt.wav`, `load(cache=False)` | `AudioError` containing `cannot decode`, no newline in the message. |
| AT-7 | A 1 s black video with no audio (made with ffmpeg's lavfi color source), `load` | `AudioError` containing `has no audio track`. Skip when ffmpeg cannot make it. |
| AT-8 | A 1 s sine with sample 24000 set to NaN, then +Inf, then -Inf, `load(cache=False)` | Each raises `AudioError` matching `1 NaN / Inf sample(s), first at 0.500 s`. |
| AT-9 | `check_paths([good.wav, missing/nope.wav])` | `AudioError` naming `nope.wav`; `check_paths([good.wav])` passes. |
| AT-10 | `replace_quietly(tmp, dst)` to a valid target; then to a target in a missing folder with `tries=1` | First returns True. Second returns False, raises nothing, and tmp no longer exists. |
| AT-11 | `emit({"summary": "ok", "file": "キー音.wav"}, args with json=True)` into a cp1252 text stream | The bytes decode as cp1252 and parse as JSON; `file` equals `キー音.wav`. |
| AT-12 | `cli.main(["measure", f, "--light", "--threads", "2"])` with `HARK_THREADS=3`; then `cli.main(["measure", f + ".nope"])` | First returns 0 and `HARK_THREADS` is `2`. Second returns 1; stderr is one line starting `hark measure: error:`. |
| AT-13 | Fresh Python process, numpy not imported, run the CLI with `measure f --light` plus: `--threads 2`; `--threads=2`; no flag with `HARK_THREADS=3`; `--threads 2` with `OPENBLAS_NUM_THREADS=1` | `OPENBLAS_NUM_THREADS` is `2`, `2`, `3`, `1`. When threadpoolctl is present and BLAS is OpenBLAS, the live thread count matches. |
| AT-14 | `hark.__version__`, the project metadata version, the newest changelog heading | All three are equal. |
| AT-15 | Fresh process: `import hark; hark.measure(x)`; then `import hark.describe` (ML) and call `hark.describe(path)` | Both work; the second returns the describe result, not a "module is not callable" error. `import hark` imports no torch. |
| AT-16 | `hark.api.__all__` | Equals the sorted list of the 22 names of 2.5. Unknown attribute raises `AttributeError`. |
| AT-17 | Each pin in config | Revision is 40 lowercase hex; weights is `model.safetensors`. |
| AT-18 | For each offline flag and values `1`, `true`, ` YES `, `on`, `0`, empty | `offline()` returns the flag name for the first four and None for the last two, read at call time (set after import). |
| AT-19 | Scan the hark source tree | No code assigns, deletes or pops `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE` or `HARK_OFFLINE` in the environment. |
| AT-20 | A fake hub cache with a complete pinned CLAP snapshot | `resolve` gives state `cached`, `snapshot` = that folder, `pinned` true, `cache_name` = `laion--larger_clap_general`. Empty cache: `missing`. A pinned folder without weights: `incomplete`, `missing` contains `model.safetensors`. |
| AT-21 | A fake cache with only the complete legacy snapshot | `resolve(id)` gives `legacy`, weights `pytorch_model.bin`; `resolve(id, pin)` (explicit revision) gives `missing`. A legacy snapshot that lacks a tokenizer file: `missing`. |
| AT-22 | A custom id with `refs/main` pointing at a snapshot that has `config.json` and `model.safetensors` | `cached`; `pinned` false; `cache_tag` = `--` + first 8 of that commit. |
| AT-23 | A fake snapshot download that records calls; offline flag set; model missing | `ensure` raises a `RuntimeError` naming the flag; the fake was never called. Without the flag: one call, with the allow-list = pin files + `model.safetensors` (no `.bin`). A custom model: the allow-list contains no `*` pattern and no `.bin`. |
| AT-24 | `HARK_CLAP` = a temp folder with `config.json` and `model.safetensors`, any offline flag | Resolves to `cached`, revision `local`, `folder` true; no download call; `cache_name` starts `local--` and contains no path separator or drive letter. Folder without weights: `ensure` raises the never-download error. |
| AT-25 | `doctor.check()` on a fake cache with CLAP cached and AST missing, no offline flag | `models` shows both states; `models_cached` CLAP true, AST false; a note says not cached yet; `problems` has no model entry; exit 0 from the CLI. With `HARK_OFFLINE=1`: a problem names the flag. |
| AT-26 | `doctor.check(load=True)` with a fake model loader that raises `RuntimeError("boom")` | No exception; `problems` contains `model load failed: boom`. |
| AT-27 | `doctor.check(fetch_missing=True)` with the fake download, CLAP cached, AST missing | `fetched` CLAP `already cached`, AST `downloaded <7 hex>`; one download call. |
| AT-28 | `measure` of a 3 s file of 1 kHz sine bursts | `lufs`, `true_peak_dbtp`, `bands_db` (7 keys), `bandwidth_hz`, `brightness_hz`, `onsets` present; `short_term_max_lufs` present; `lra_lu` absent. |
| AT-29 | EBU Tech 3341 minimum-requirement case: stereo 1 kHz sine at -23 dBFS in both channels, 20 s, `integrated_lufs` | -23.0 LUFS within 0.1 LU. Add the other Tech 3341 integrated cases the same way. |
| AT-30 | 10 s digital silence | `peak_dbfs` -240.0; `lufs` -70.0; summary starts `<name>: 10.000 s, digitally silent (peak -240 dBFS): nothing to measure`. |
| AT-31 | `hark measure a.wav b.wav --json` | stdout is one JSON object with `summary` `2 files measured.` and `files` of length 2, each with `file` and `summary`. |
| AT-32 | `hark measure a.wav --out sub/dir/r.json` | `sub/dir` is made; the file parses as JSON and equals the `--json` output. |
| AT-33 | `list_media` on a glob that matches 2 folders and 1 WAV; on a folder with nested WAVs and a `.txt` | The glob yields only the WAV. The folder yields the nested WAVs in sorted order and no `.txt`. |
| AT-34 | `probe` of a stereo 44.1 kHz MP3 with cover art | `codec` `mp3`, `src_rate` 44100, `channels` 2, `lossy` true, `has_video` false, `kbps` an int. |
| AT-35 (ML) | CLAP on a 25 s signal, `embed_audio(per_window=True)` | 4 windows (starts 0, 5, 10, 15 s; the last is flush with the end at 15 s); the default output is one unit vector (norm within 1e-5 of 1). |
| AT-36 (ML) | `embed_file(path)` twice | The second call reads the cache (no model call) and returns the same vector bit for bit. |
| AT-37 (ML) | `embed_text(["a", "b", "a"])` | Shape (3, 512); rows 0 and 2 are equal; `text.npz` holds 2 keys. |
| AT-38 (ML) | AST on a 30 s signal | Window starts 0, 5.12, 10.24, 15.36 s plus one flush with the end at 19.76 s; output (527,), values in [0, 1]; `top(p, 5)` sorted descending. |
| AT-39 | `hark -h` with one command module made to fail at import | Exit 0; stderr has the module note; help still lists the other commands. |
| AT-40 | Run an ML command with torch hidden (import spec missing), stdin not a TTY | Exit 1; one stderr line that contains the exact install command; no prompt. |

Note on AT-29: the EBU Tech 3341 and 3342 documents publish the loudness and loudness-range test cases. They are
public facts and give exact expected values for 4.14.
