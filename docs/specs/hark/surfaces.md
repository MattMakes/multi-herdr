# hark spec: surfaces (gallery, MCP server, LLM critique, match, limits, integration)

Unit: `surfaces`. Side of the writer: contaminated (spec writer). Readers: clean implementers.
This file is the behaviour spec. A clean implementer rebuilds the unit from it alone.

The source covers these subjects in this unit:

| subject | what it is |
|---|---|
| gallery | one static HTML listening page built from forge output folders |
| MCP server | the engine's listening, review and bank functions exposed as MCP tools over stdio |
| LLM critique | an optional second opinion in words from a hosted audio language model |
| match | how well a sound fits one or more text descriptions, with calibrated z and a verdict |
| integration | how a code-rendered film pipeline uses the bank, the kit and the listening tools |
| limits and validation | what the engine can and cannot judge, and every measured result |

Names. hark uses its own names everywhere:

| thing | hark name |
|---|---|
| package, CLI, MCP server name | `hark` |
| CLI subcommands of this unit | `hark gallery`, `hark mcp`, `hark llm`, `hark match` |
| thread setting | `HARK_THREADS` |
| cache root | `HARK_HOME` (default `~/.cache/hark`) |
| reference registry override | `HARK_REFSETS` |
| LLM model override | `HARK_LLM_MODEL` (see Q3) |
| real-recordings skill | `sound-sourcing` |
| tool annotation title | `hark <tool name>` |

"The source" means the upstream project. "The forge" means hark's sound maker (unit forge-core).
"The bank" means hark's shipped sound bank (unit bank-kit-sfx).

### Parts

The spec is longer than 60 KB, so it is split. This file is the index. Read all 5 files.

| file | content |
|---|---|
| `surfaces.md` (this file) | 1 Interface (exact, every subject), 2 section map, 3 Data assets, 4 Dependencies on other units, 5 Acceptance tests (index), 6 Open questions |
| `surfaces-gallery.md` | part G: gallery Behaviour, Algorithms, Errors and edge cases, Constants, Acceptance tests |
| `surfaces-mcp.md` | part M: MCP server and match Behaviour, Algorithms, Constants, Errors and edge cases, Acceptance tests |
| `surfaces-critique.md` | part C: LLM critique Behaviour, source transport, agy mapping, Algorithms, Constants, Errors and edge cases, Acceptance tests |
| `surfaces-limits.md` | part L: every documented limit and validation result, the film-pipeline integration, Acceptance tests |

---

## 1. Interface

### 1.1 `hark gallery`

CLI:

```
hark gallery [FOLDER] [-o HTML] [--results [LABEL:]JSON ...] [--refs forge|DIR]
             [--compare LABEL:GLOB ...] [--sub NAME] [--alias OLD=NEW ...]
             [--embed] [--embed-compressed] [--embed-codec flac|wav|mp3|opus|vorbis]
             [--kbps N] [--max-mb F] [--title TEXT] [--max-refs N]
             [--json] [--out FILE] [--threads N]
```

| flag | type | default | meaning |
|---|---|---|---|
| `FOLDER` | path, optional | none | a forge output tree (`<category>/<sub>/*.wav`), one category folder, or one final folder |
| `-o` | path | `gallery.html` | the output HTML; thumbnails go to `<stem>_files/` beside it |
| `--results` | 1 or more strings | none | benchmark results JSON files; with `--compare` each must be `LABEL:PATH` |
| `--refs` | `forge` or a folder | none | add an A/B row of forge-side references; `bench` is refused |
| `--compare` | 1 or more `LABEL:GLOB` | none | versions shown as rows; the user quotes the globs so the shell does not expand them |
| `--sub` | string | `final` | the subfolder per category that holds the sounds |
| `--alias` | 1 or more `OLD=NEW` | none | show the folder category OLD on the card of category NEW |
| `--embed` | flag | off | put audio and images inside the HTML as base64 data URIs |
| `--embed-compressed` | flag | off | one small self-contained page with lossy previews (implies `--embed`) |
| `--embed-codec` | choice | none (see surfaces-gallery.md G2.10) | codec of embedded audio |
| `--kbps` | int | per codec | bit rate of lossy previews, 16 to 320 |
| `--max-mb` | float | 25.0 | add a warning when an embedded page is larger than this |
| `--title` | string | none | page title |
| `--max-refs` | int | 6 | at most this many reference tiles per category |
| `--json`, `--out`, `--threads` | common flags | | full JSON on stdout; JSON also to a file; CPU threads |

If neither `FOLDER` nor `--compare` is given, the command fails with an error (exit 1).

Python:

```
gallery(folder=None, out='gallery.html', results=None, refs=None, compare=None,
        embed=False, codec='flac', sub='final', title=None, max_refs=6,
        compressed=False, kbps=None, max_mb=25.0, alias=None) -> dict
```

`results` can be one string or a list. `alias` can be a dict or a list of `OLD=NEW` strings.

Return value (all keys always present unless marked):

| key | type | meaning |
|---|---|---|
| `html` | str | absolute path of the page, forward slashes |
| `assets` | str | absolute path of `<stem>_files/`, forward slashes |
| `url` | str | `file://` URI of the page |
| `versions` | dict label -> int | number of categories found per version |
| `categories` | int | number of cards on the page |
| `sounds` | int | number of made sounds on the page (references not counted) |
| `refs` | int | number of reference tiles on the page |
| `images` | dict | `made` (new thumbnails), `reused` (cache hits), `pruned` (stale thumbnails removed) |
| `embedded` | bool | true when audio is embedded |
| `codec` | str or null | `null` when not embedded; `flac` or `wav`; for lossy `"<codec> <kbps> kbps"` |
| `size_mb` | float | page size in MB (10^6 bytes), 2 decimals |
| `critic_pass` | dict label -> [passed, total] | critic pass counts per version |
| `seconds` | float | build time, 1 decimal |
| `warnings` | list of str | every note of the build, in order |
| `embedded_mb` | dict, only when embedded | `audio` and `images`: base64 size in MB, 2 decimals (raw bytes x 4/3 / 10^6) |
| `text` | str | the human summary (see surfaces-gallery.md G1.15) |

### 1.2 `hark mcp`

CLI:

```
hark mcp [--warm] [--tools] [--json] [--out FILE] [--threads N]
python -m hark.<mcp module> [--warm]      # the same server without the CLI front end
```

- No flag: start the MCP server on stdio. The MCP client starts it. It is not run by hand.
- `--warm`: also load the models in a background thread at start.
- `--tools`: print the tool list and exit 0. The server does not start.

Server identity: name `hark`, version = the hark package version, log level WARNING, and the
instructions text of surfaces-mcp.md M1.9.

Registration examples that the docs give (hark names):

```
claude mcp add hark --scope user -e HARK_THREADS=3 -- hark mcp
{"mcpServers": {"hark": {"command": "<launcher>", "args": ["mcp"], "env": {"HARK_THREADS": "3"}}}}
```

Tools. 16 tools in this order. "ro" = readOnlyHint. "net" = openWorldHint. destructiveHint is false for all.

| # | tool | ro | net | backing function (unit) |
|---|---|---|---|---|
| 1 | `measure` | yes | no | features measure (core-io) |
| 2 | `describe` | yes | no | describe (listen) |
| 3 | `match` | yes | no | match (this unit, 1.4) |
| 4 | `rank` | yes | no | rank (listen) |
| 5 | `find` | yes | no | find (listen) |
| 6 | `compare` | yes | no | compare (listen) |
| 7 | `ab` | yes | no | ab (listen) |
| 8 | `realness` | yes | no | realness, realness_many (listen) |
| 9 | `timeline` | yes | no | timeline (timeline) |
| 10 | `sheet` | no | no | sheet (timeline) |
| 11 | `critique` | yes | no | critique (timeline) |
| 12 | `doctor` | yes | no | doctor check (core-io) |
| 13 | `llm_critique` | no | yes | LLM critique (this unit, 1.3) |
| 14 | `sfx_list` | yes | no | bank sfx list (bank-kit-sfx) |
| 15 | `sfx_render` | no | no | bank sfx render (bank-kit-sfx) |
| 16 | `kit_render` | no | no | kit render (bank-kit-sfx) |

Shared argument types (JSON schema):

| alias | schema | meaning |
|---|---|---|
| Path | string | absolute path to an audio file or a video file (the audio track is used) |
| Paths | string or array of strings | files, folders (searched recursively) or globs |
| Full | boolean, default false | return the full JSON instead of the compact JSON |
| Start | number or null, default null | start of the segment to analyse, seconds |
| End | number or null, default null | end of the segment, seconds |
| Query | string | a plain description of the sound itself, not of its use |
| Video | string or null, default null | path of the film, for picture cuts and frames |
| Cues | array of numbers, or string, or null | cue times as seconds, as `"1.2,3.4"`, or a path to a cues JSON file `{fps, events: [{f or t, kind}]}` |

Every tool has `full: Full`. Tool arguments (required ones have no default):

| tool | arguments (name: type = default; bounds) |
|---|---|
| `measure` | `path`: Path; `start`; `end`; `music`: bool = false (adds tempo, beats, key); `light`: bool = false (levels and spectrum only) |
| `describe` | `path`; `start`; `end`; `top`: int = 8, 1..30 (tags per category) |
| `match` | `path`; `query`: Query; `alts`: array of string or null = null (1 to 5 rival descriptions); `start`; `end` |
| `rank` | `paths`: Paths; `query`: Query; `top`: int or null = 20, >= 1 |
| `find` | `path`; `query`; `top`: int = 5, 1..50; `kind`: string = `auto` (`auto`, `event`, `texture`); `win`: number or null (texture window s; default duration/6 clamped to 1..5 s); `hop`: number or null; `start`; `end` |
| `compare` | `path`; `refs`: string or array (files, folders, globs); `k`: int = 5, 1..50; `start`; `end` |
| `ab` | `a`: string; `b`: string; `query`: string or null; `refs`: string, array or null; `a_start`: number or null, >= 0; `a_end`: number or null, > 0; `b_start`: >= 0; `b_end`: > 0 |
| `realness` | `path`: string (a file, a folder or a glob); `start`; `end`; `ast`: bool = false (adds AudioSet synth-class evidence) |
| `timeline` | `path`; `video`: Video; `cues`: Cues; `start`; `end`; `detail`: string = `normal` (`low`, `normal`, `high`) |
| `sheet` | `path`; `video`; `cues`; `start`; `end`; `detail` = `normal`; `out_png`: string or null; `include_image`: bool = true |
| `critique` | `path`; `video`; `profile`: string or null (a profile JSON); `fast`: bool = false (skip the AudioSet and realness passes) |
| `doctor` | `load`: bool = false (also load both models and time one window) |
| `llm_critique` | `path`; `consent`: bool = false; `prompt`: string or null; `video`; `with_timeline`: bool = false; `model`: string or null; `synth_intended`: bool = false; `start`; `end` |
| `sfx_list` | `category`: string or null; `kind`: string or null |
| `sfx_render` | `sound`: string (bank id, e.g. `whoosh_fast/crisp`); `out`: string (absolute WAV path, 24-bit); `seed`: int or null; `dur`: number or null, > 0; `apex`: number or null, > 0; `pitch`: string or null (note name such as `E5`, `Eb6`, or Hz such as `440`); `peak_db`: number = -1.0, <= 0 |
| `kit_render` | `kit`: string (absolute path of kit.json); `out`: string (absolute folder); `force`: bool = false |

Tool result. Every tool returns MCP text content. `sheet` with `include_image` true returns 2 content
items: the text, then one JPEG image. A failure returns an MCP tool error (isError true) with one readable
message. The text layout is in surfaces-mcp.md M1.4.

The JSON part of each tool is the result dict of its backing function. Those fields are defined by the
owning unit (see the table above), except `measure` (adds `summary`, M2.2), `realness` on many files (adds
`text`, M1.8.1), `sfx_list` (adds `text`), `match` (index 1.4) and `llm_critique` (index 1.3).

Each tool description is 1 paragraph, whitespace collapsed to single spaces. The implementer writes its own
descriptions. Each one says: what the tool returns, when to use it, how long it can take, and what to trust.
Required content per tool is in surfaces-mcp.md M1.8.

`--tools` output: JSON `{summary, tools: [{name, read_only, network, description}]}`. Text: a head line
with the tool count and a registration command, then 1 line per tool: the name padded to 13 characters,
then the first sentence of the description, with a period.

### 1.3 `hark llm` (LLM critique)

CLI:

```
hark llm FILE [--prompt TEXT] [--yes-upload] [--model ID] [--video PATH] [--with-timeline]
              [--no-cuts] [--start S] [--end S] [--max-seconds S] [--synth-intended]
              [--json] [--out FILE] [--threads N]
```

| flag | default | meaning |
|---|---|---|
| `FILE` | required | soundtrack, stem or video; only the audio is sent |
| `--prompt` | none | an extra question for the model to answer |
| `--yes-upload` | off | the consent: the user agrees that this audio leaves the machine |
| `--model` | `HARK_LLM_MODEL`, else the provider default | model id |
| `--video` | none | the film, for picture-cut context; only cut times are sent, never frames |
| `--with-timeline` | off | also send the listening transcript (loads models) |
| `--no-cuts` | off | do not compute picture cuts |
| `--start`, `--end` | none | send only this part of the file, seconds |
| `--max-seconds` | 600 | trim longer audio to this length |
| `--synth-intended` | off | the film uses synthesized sound on purpose; synthesis is not a flaw |

Exit code: 0 when the result has `ok` true, else 1 (also when the critique is off or not consented).

Python:

```
critique(path, prompt=None, provider=<default>, model=None, consent=False, video=None,
         with_timeline=False, start=None, end=None, max_seconds=600.0, cuts=True,
         synth_intended=False) -> dict
encode(path, start=None, end=None, max_seconds=600.0, kbps=64) -> (bytes, mime, info)
context(path, clip, video=None, with_timeline=False, cuts=True) -> (text, notes)
build_prompt(ctx_text, question=None, synth_intended=False) -> str
parse_json(text) -> dict or None
normalize(obj, offset=0.0, clip_len=1e9) -> dict
render(result) -> str
```

Result when the critique is off (provider not ready, or no consent):

| key | value |
|---|---|
| `enabled` | false |
| `ok` | false |
| `path`, `provider`, `model` | the file, the provider name, the model id that would be used |
| `summary` | why it is off |
| `how_to_enable` | list of steps (strings) |
| `text` | summary, then the steps numbered `  1. ...` on separate lines |

Result when a step fails after consent: `enabled` true, `ok` false, `path`, `provider`, `model`,
`uploaded` (the clip info, or null when encoding failed), `summary` and `text` both
`"LLM critique failed: <message>"`, `error` = the message.

Result on success (the model answered):

| key | meaning |
|---|---|
| `enabled` | true |
| `ok` | true when the answer parsed as a JSON object, else false |
| `path`, `provider` | as given |
| `model` | the model version that the provider reports, else the requested id |
| `uploaded` | clip info (below) plus `method`, plus `files_api_deleted` when the provider reports it |
| `seconds` | wall time of the call, 1 decimal |
| `usage` | token counts that the provider reports: `promptTokenCount`, `candidatesTokenCount`, `thoughtsTokenCount`, `totalTokenCount` (only the present ones) |
| `finish_reason` | the provider's finish reason |
| `context_sent` | the exact context text in the prompt |
| `notes` | list of strings: context that was not sent and why |
| `critique` | the normalized answer (below), only when parsed |
| `raw` | the raw answer, at most 20000 characters, only when not parsed |
| `summary`, `text` | see surfaces-critique.md C4.3 and C4.4 |

Clip info (`encode` third value): `start`, `end`, `seconds` (all rounded to 3 decimals), `file_duration`
(3 decimals), `trimmed` (bool), `bytes` (int), `codec` (`"mp3 mono 64 kbps"`). The mime is `audio/mp3`.

The answer schema that the prompt asks for (exact key names):

```
{"overall": {"score": 1-10, "verdict": "one sentence"},
 "scores": {"sync": 1-10 or null, "realism": 1-10, "mix_balance": 1-10, "loudness": 1-10,
            "music": 1-10 or null},
 "notes": {"sync": "", "realism": "", "mix_balance": "", "loudness": "", "music": ""},
 "synthetic_or_cheap": [{"start": s, "end": s, "what": "", "why": ""}],
 "issues": [{"start": s, "end": s, "severity": "high|medium|low",
             "category": "sync|realism|mix|loudness|music|noise|edit", "problem": "", "fix": ""}],
 "strengths": [""],
 "top_fixes": ["at most 5, most important first"],
 "answer": "only when the editor asked a question"}
```

The normalized `critique` (output of `normalize`):

| key | type |
|---|---|
| `overall` | `{score: float 0..10 1 decimal or null, verdict: str}` |
| `scores` | dict with exactly `sync`, `realism`, `mix_balance`, `loudness`, `music`: float or null |
| `notes` | dict of the same keys, only the non-empty ones |
| `synthetic_or_cheap` | list of `{start, end, what, why}`, times in file seconds |
| `issues` | list of `{start, end, severity, category, problem, fix}`, sorted (surfaces-critique.md C4.2) |
| `strengths` | at most 8 non-empty strings |
| `top_fixes` | at most 5 non-empty strings |
| `answer` | str, only when the model gave one |

Provider contract. A provider is an object with: `name`; a readiness check (source: an API key in the
environment); a default model; `model(model=None)` that returns the id without a leading `models/`; and
`generate(audio_bytes, mime, prompt, model) -> {text, finish_reason, method, files_api_deleted,
model_version, usage}`. `generate` raises a provider error with a readable message on failure.
The hark provider uses the Antigravity CLI. See surfaces-critique.md C6.

### 1.4 `hark match`

CLI:

```
hark match PATH QUERY [QUERY ...] [--alts ALT ...] [--start S] [--end S] [--json] [--out FILE] [--threads N]
```

Python: `match(path, query, alts=None, start=None, end=None) -> dict`. `query` is a string or a list of
strings.

Result keys:

| key | meaning |
|---|---|
| `summary` | the sentence of surfaces-mcp.md M3.3 |
| `path`, `start`, `end` | as given |
| `peak_dbfs` | sample peak of the (segment of the) file, stereo decode, 1 decimal |
| `query`, `cosine`, `z`, `percentile`, `rank_in_vocab`, `p_vs_alts`, `verdict`, `validated` | the fields of the best query (highest z) |
| `alts` | only when alternatives were given: the best query's `[{query, p, z, cosine}]` |
| `results` | every query row, sorted by z, highest first |
| `closest_vocab` | 4 nearest vocabulary terms `[{term, category, z, cosine}]` |
| `verdict_thresholds_z` | `{"strong": 3.0, "plausible": 2.5, "weak": 2.0}` |
| `verdict_evidence` | the evidence table of surfaces-mcp.md M5, as a dict keyed by verdict |

One query row: `query`, `cosine` (3 decimals), `z` (2 decimals), `percentile` (1 decimal, 0..100),
`rank_in_vocab` (int, from 1), `vocab_size` (int), `verdict`, `validated` (bool), `p_vs_alts`
(3 decimals or null), and `alts` when alternatives were given.

### 1.5 Integration contracts (film pipeline)

hark guarantees these to a film pipeline that calls it (details in surfaces-limits.md L3):

- the kit role vocabulary of surfaces-limits.md L3.2;
- the kit file and lock file formats, schema 1 (owned by unit bank-kit-sfx);
- the CLI flags `hark kit init --preset calm|crisp|minimal|launch` and `hark kit render KIT --out DIR`;
- deterministic takes: the same kit, bank version and seeds give the same bytes;
- a lock that names the bank version; a new bank version changes every take's cache key, so the kit renders again.

The cue file that a film pipeline exports and that hark reads (`--cues`):
`{fps, bpm, total, acts, cue, events: [{f, kind, weight, pan, pitch?, apexFrac?, dur?, variant?, id?}]}`.
`f` is a frame number. The time of an event is `f / fps`.

---

## 2. Section map

Each required section is present for every subject. Use this map to find it.

| section | gallery | MCP server | match | LLM critique | limits, validation, integration |
|---|---|---|---|---|---|
| Interface | 1.1 | 1.2 | 1.4 | 1.3 | 1.5 |
| Behaviour | G1 | M1 | M3 | C1 to C4, C6 | L1, L2, L3 |
| Algorithms | G2 | M2 | M4 | C7.1 | (none: facts and contracts only) |
| Constants | G4 | M5 | M5 | C7.2 | numbers inside L1 and L2 |
| Data assets | 3 | 3 | 3 | 3 | 3 |
| Errors and edge cases | G3 | M6 | M6 | C8 | (none) |
| Acceptance tests | G5 | M7 | M7 | C9 | L4 |
| Dependencies | 4 | 4 | 4 | 4 | 4 |
| Open questions | 6 | 6 | 6 | 6 | 6 |

## 3. Data assets

| asset | used by | format and meaning | origin | mark | fallback |
|---|---|---|---|---|---|
| family table (keys, member lists, keywords) | gallery | table G1.4: family key, ordered category list, keyword list | written by hand for the forge categories | DERIVE | none needed |
| family titles and lines, `pro_` descriptions, legend and lede text | gallery | short display strings | hark writes its own from the facts in G1.4, G1.13 and G4.3 | DERIVE | none needed |
| category descriptions | gallery | dict category -> 1 sentence, from the forge reference-set module | unit forge-core | DERIVE | empty description |
| K-weighting coefficients | gallery | 2 biquads at 48 kHz (G4.1) | ITU-R BS.1770, public | DERIVE | none needed |
| colour map stops | gallery | 6 stops (G4.1) | design constants | REGENERATE | any monotonic white-to-ink map |
| reference-set registry (`refsets.json` under `HARK_HOME`, or `HARK_REFSETS`) | gallery `--refs`, held-out guard | `{version, categories: {cat: {forge: [paths], bench: [paths], note}}}` | made from licensed recordings laid out as `<root>/<category>/{forge,bench}/*.wav` (unit forge-core) | NEEDS-DATA | no `--refs` row; the held-out guard treats a missing registry as an empty bench set and still applies the kit-bench stem list |
| kit-bench stem list | held-out guard | 8 file stems (G4.4) | unit forge-core | DERIVE | none needed |
| benchmark results JSON | gallery `--results` | `{tag, created, categories: {cat: {made: [{file, length_s, tone_db, t40_ratio, typ_z, phrase_z, stereo_corr, artifacts, as_top, artifact_detail: {peak_dbfs}}], summary: {made, real_cross, real_loo: {var_cos, t40_ratio, tone_db, n, artifacts}}, inside: {per_sound, real_per_sound, n_checks}, n_bench, n_forge}}}` | the forge benchmark (unit forge-core) over held-out bench recordings | NEEDS-DATA | build without `--results`; tests use a small synthetic results JSON |
| forge report (`report.json`) and audition index (`audition.txt`) | gallery | report fields of G1.5; index lines `<seconds> s ref <name>` | written by the forge | REGENERATE | sounds still show without report data; no audition reel |
| vocabulary terms and templates (`vocab.json`) | match, describe | categories -> terms, plus 5 prompt templates; 322 terms | unit listen | DERIVE (from public sound words and the AudioSet label list) | unit listen's fallback |
| vocabulary reference statistics (`vocab_stats.json`) | match | the mean audio embedding m over the reference recordings, the CLAP logit scale, the model id | the source built it from 241 reference recordings (unit listen) | NEEDS-DATA | unit listen: rebuild from any local set of real recordings, or use m = 0 (no bias removal) and read the logit scale from the model |
| verdict evidence table | match | table M5 | measured on 125 held-out real recordings | NEEDS-DATA to measure again; the numbers ship as documented facts | quote the numbers as source measurements |
| realness model file | MCP `realness` | unit listen | unit listen | (see unit listen) | the tool error of M1.8.1 |
| launch profile | MCP `critique` | unit timeline | unit timeline | (see unit timeline) | (see unit timeline) |
| LLM rubric and intent statement | LLM critique | prompt text (C3.2, C3.3) | hark writes its own text from C3.2 | DERIVE | none needed |
| agy probe verdict and invocation | LLM critique | `docs/specs/hark/agy-audio-probe.md` | ticket 11 | REGENERATE (run the probe) | when absent or DOES-NOT-HEAR: exit 2, nothing sent |
| validation numbers | docs | tables L2 | source measurements on licensed data and third-party films | NEEDS-DATA | quote as source measurements; hark re-measures only what its own synthetic fixtures can test |

## 4. Dependencies on other units

| unit | what surfaces needs from it |
|---|---|
| core-io | decode (`load`, rejects NaN and infinity), `duration`, `probe`, media listing (extensions, folders, globs of files only), `file_id`, video extensions, `HARK_HOME`, thread setting and BLAS hold, the public API name -> module table, the common CLI flags (`--json`, `--out`, `--threads`), the 1-line error format `hark <command>: error: <msg>`, result emit (text unless `--json`; `--out` writes JSON), result cleaning (NaN to null), `measure` keys (`duration`, `error`, `peak_dbfs`, `digital_silence_frac`, `lufs`, `short_term_max_lufs`, `rms_dbfs`, `true_peak_dbtp`, `clipped_samples`, `lra_lu`, `noise_floor_dbfs`, `brightness_hz`, `bandwidth_hz`, `attack_ms`, `decay_ms`, `onset_count`, `tempo`, `key`), `doctor` check, the models (CLAP, AST) and the ML-extras prompt |
| listen | vocabulary (`get`, `scores`, `top_terms`, `nearest_category`, `embed_text`, the logit scale, `check`, `embed_file`), `describe`, `rank`, `find`, `compare`, `ab`, `realness`, `realness_many`, the realness model path |
| timeline | `timeline` (text, summary; `detail`), `sheet` (writes a PNG, returns `png`), `critique` (`use_ast`, `use_realness`, `profile`), the video cut detector |
| forge-core | reference-set registry (`refset`, `load`, `held_out`, the kit-bench stems, category descriptions, the missing-registry error), forge report and audition files, the benchmark results JSON, the forge output layout `<category>/final/*.wav` |
| recipes-impacts, recipes-motion | the category names that the gallery groups (G1.4) |
| bank-kit-sfx | `sfx_list` (summary, sounds with `id`, `dur_s`, `peak_s`, `kinds`, `knobs: {dur, apex, pitch: {note}}`), `sfx_render`, `kit_render`, `kit init`, `kit check`, kit and lock schema 1, mix keys |
| fit | none directly (fitted sounds reach the gallery and the bank as normal sounds) |
| skills-and-docs | states the limits and validation facts of part L, the MCP registration, and the `sound-sourcing` mixer and its QA |
| surfaces (this unit) | `match` is also listed under unit listen's source files: the listen spec owns the calibration, this spec owns the match command, its result and its texts (see Q1) |

## 5. Acceptance tests (index)

| part | ids | count |
|---|---|---|
| gallery | G-T1 to G-T20 | 20 |
| MCP server and match | M-T1 to M-T23 | 23 |
| LLM critique | C-T1 to C-T20 | 20 |
| limits and integration | L-T1 to L-T5 | 5 |

## 6. Open questions

| id | question | proposed answer |
|---|---|---|
| Q1 | `match` is listed as a source file of both unit listen and this unit. Which spec owns it? | this spec owns the command, its result and its texts; the listen spec owns the vocabulary and the calibration math (M4 restates it only as a contract) |
| Q2 | The source tooltip of the card span says the thumbnails have an 80 dB range, but the drawing uses 72 dB. | use 72 dB everywhere, also in the tooltip |
| Q3 | Name of the LLM model override. The source has a vendor-specific variable. | `HARK_LLM_MODEL`; when unset, pass no `--model` to agy |
| Q4 | How agy takes an audio file (path in the prompt with `--add-dir`, or a stream-json attachment), and which output field holds the answer text. | use what `docs/specs/hark/agy-audio-probe.md` says works best; C6 holds the default form |
| Q5 | hark does not have the source's licensed data. How do hark docs quote the validation numbers of L2? | quote them as source measurements with that label, or leave them out; never claim them as hark results |
| Q6 | The film-pipeline bridge finds the engine by an environment variable that names its python. hark name? | `HARK_PYTHON` |
| Q7 | Should hark keep the gallery's default page titles? | no: hark uses its own titles (G1.13) |
| Q8 | The source MCP server counts 13 listening tools in its tests and 16 in all. Must hark test all 16 over stdio? | yes: M-T2 lists all 16; the 3 bank tools need no models and run fast |
| Q9 | The borrowing mark (`same_as` from another results file) and the identical-bytes mark can both apply to 1 sound; the later write wins in the source. | keep that order: the identical-bytes mark is set last |
| Q10 | Retries for agy: the source retries HTTP 429 and 5xx. agy exit codes are not known yet. | retry once after 4 s only when agy's output marks a rate limit; else fail at once |
