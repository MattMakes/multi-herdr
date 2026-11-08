# hark spec: surfaces, part C (LLM critique)

Part of `docs/specs/hark/surfaces.md`. The interface of `hark llm` and the MCP tool `llm_critique` is in the
index, section 1.3. Section numbers here start with C.

Source behaviour versus hark behaviour. The source calls a hosted audio model through its HTTP API with an API
key from the environment. hark never uses an API key. hark calls the Antigravity CLI `agy -p`, which uses the
operator's own sign-in. C1 to C4 describe the flow that both share. C5 describes the source transport as a
reference, and C6 gives the hark mapping to agy. Whether agy hears audio at all is decided by the probe spec
`docs/specs/hark/agy-audio-probe.md` (ticket 11).

## C1. Behaviour: gating and consent

The critique is off unless 2 conditions hold:
1. The provider is ready. Source: an API key in the environment (2 names checked in order; a value that
   starts with `${` is an unexpanded placeholder from an MCP config and counts as no key). hark: `agy` is on
   PATH and the probe verdict says it hears audio (C6.1).
2. The user agrees to the upload, and the caller passes that consent: `--yes-upload` on the CLI, `consent=True` in Python and MCP. The audio
   leaves this machine and goes to the model vendor (Google, also through agy).

Order of checks:
1. An unknown provider name is a value error that lists the providers.
2. A path that is not a file is an audio error (no such file).
3. Provider not ready: return the off result (index 1.3). Its summary says the critique is off and that the
   rest of hark works without it. Its steps say how to make the provider ready. Source steps: get a key from the user's own vendor account (use
   may cost money); export it in the shell of the tool and never write it into code, files or a chat; run again
   with consent when the user accepts the upload. hark steps: install agy and sign in; rerun
   with `--yes-upload` only when the user agrees to the upload.
4. No consent: return the off result. Its summary says nothing was uploaded, names the file and the model, and
   says explicit consent is needed. Its 2 steps: get the user's permission to send this audio to the
   vendor for an AI review; with permission, run again with `--yes-upload` (CLI) or `consent=True` (Python, MCP).
5. Else run C2 to C4. Any exception in encode, context or generate becomes the failure result (index 1.3).
   The message is scrubbed: any secret value (source: the key) is replaced by `***`. A failure never shows a
   traceback.

The upload holds audio only. Frames of a video are never sent; a video adds cut times as text and nothing else.

The rest of hark never needs the critique.

## C2. Behaviour: the clip

`encode(path, start, end, max_seconds, kbps=64)`:
1. dur = the container duration from the probe (unit core-io), else the decoded duration.
2. a = max(0, start or 0). b = min(dur, end) when end is given, else dur.
3. When b <= a: error `empty range <a>-<b> s (file is <dur> s)` (3 decimals).
4. When b - a > max_seconds: b = a + max_seconds and `trimmed` = true.
5. ffmpeg: input seek to a (3 decimals) before the input, duration b - a, no video, 1 channel, 44100 Hz,
   libmp3lame at `<kbps>k`, MP3 format, to stdout, no stdin, errors only.
6. A non-zero exit or empty output is an error naming the file and the first 300 characters of stderr.
7. Return the bytes, mime `audio/mp3`, and the clip info of index 1.3.

## C3. Behaviour: context and prompt

### C3.1 Context text

`context(path, clip, video, with_timeline, cuts)` returns (text, notes). Lines, in order:

1. Clip line: the clip length (1 decimal) in seconds, the file seconds it covers (1 decimal each), the file
   length (1 decimal), and the codec text, all in 1 sentence.
2. Measured line: run the light measurement (unit core-io) on the clip range. For each present key of table
   C7.2, write `<label> <value><unit>`, joined by `, `. The line says that these numbers are exact and must be
   used, never estimated. Leave the line out when no key is present.
3. Video: `video` when given, else the path itself when it has a video extension, else none.
4. With `with_timeline`: run the timeline (unit timeline) on the path with that video, the clip range and
   detail `low`. Take its `text` (else `summary`), at most 16000 characters. Add a line that introduces it as
   the machine-made listening transcript in file times, plus the clip start in file seconds when the clip does
   not start at 0, then the transcript. The transcript already holds cut times, so do not add cuts again.
   When the timeline fails, add a note that says the transcript was left out of the request and gives the error type and message.
5. Cuts: when there is a video, no transcript was added, and `cuts` is true: detect picture cuts (unit
   timeline, the video cut detector). Keep cuts c with clip start <= c < clip end, as c - clip start rounded to
   2 decimals, at most 200, in the shortest number form, joined by `, `. Add a line that names them as picture
   cuts or big visual changes in clip seconds, and says that the detector finds hard cuts and whips but misses
   or is late on slow crossfades. No line when no cut is in range. When detection fails, add the note
   note that says the cut times were left out of the request and gives the error type.

Lines are joined with newlines.

### C3.2 The rubric (prompt body)

hark writes its own rubric. The rubric gives the model these instructions. The list states the intent of each
instruction, not its words.

| # | topic | intent |
|---|---|---|
| R1 | persona | the model acts as an experienced film re-recording mixer who supervises sound; the material is the mix (or 1 stem) of a short launch film for a product |
| R2 | quality bar | compare with launch films from the best studios: deliberate sound choices, a produced music mix, effects placed on picture events, a master that is loud yet keeps its dynamics |
| R3 | origin neutrality | an effect may be a recording or a designed or synthetic sound; grade fit and execution only. Hear the full clip. Name concrete moments. Give praise only where it is earned |
| R4 | sync | judge whether accents, hits and whooshes fall on their moments. The model has no picture, so it uses the cut and cue times from the context; without them it judges rhythmic precision and whether sounds drag, rush or blur. The sync score is null when there is no reference to judge against |
| R5 | realism | name the sounds that come across as recorded and those that come across as code-made, cheap stock or generic. Examples to give: bare sine beeps, unshaped noise whooshes, chiptune blips, stock risers, 1 sample fired again and again unchanged. A synthetic sound is a fault only when it should pass as recorded. When the context or the question declares synthesis intentional, grade those sounds as sound design |
| R6 | balance | music, effects and ambience against each other: masking, elements that stick out or vanish, muddy lows, harsh 2 to 5 kHz. The clip is mono, so width is not graded |
| R7 | loudness | take loudness values from the measured context and never invent them; check for over-compression, low level, peaks, pumping and clipping |
| R8 | weak spots | the moments that an expert would find embarrassing |
| R9 | fixes | every fix has a time range and a precise action. Give examples of that precision: replace a harsh beep by a softer tap that fits; lower the music 4 dB from 12.0 to 15.0; move a hit 80 ms earlier; high-pass a pad at 150 Hz |
| R10 | score scale | integer-like 1 to 10. Anchors: 10 same as the premium bar; 7 professional with minor faults; 5 capable amateur; 3 plainly cheap; 1 broken. The realism score grades how convincing and well crafted the sounds are for the film's intent, not whether they are recordings |
| R11 | time base | numbers in seconds from the start of this clip (for example 12.4), not clock text such as 0:12 |
| R12 | output | the whole reply is 1 JSON object of the schema in index 1.3; the `answer` key appears in that schema only when the editor asked a question |

### C3.3 Prompt assembly

`build_prompt(ctx_text, question, synth_intended)`:
1. The rubric. Its schema has the `answer` key only when a question is given.
2. A blank line, the word `Context:` on its own line, then the context text.
3. With `synth_intended`: a newline, then an intent paragraph. Its intent: the procedural or synthetic sound
   of this film is deliberate; grade it as sound design; put a sound in `synthetic_or_cheap` only for poor
   craft (cheap, harsh, generic, badly made), and never for being synthetic.
4. With a question: a blank line, a line that presents the editor's question, asks for the reply in `answer`
   and says the other fields are still required, then the question text.

## C4. Behaviour: answer handling

### C4.1 Parse (`parse_json`)

1. Remove a leading code fence (optional `json` tag) and a trailing code fence.
2. Candidates: the whole text, then the outermost JSON object (C7.1).
3. For each candidate, try to load it as-is, then with trailing commas removed (a comma followed by optional
   whitespace and then `}` or `]` loses the comma).
4. Return the first result that is a JSON object. Else return null.

### C4.2 Normalize

`normalize(obj, offset = clip start, clip_len = clip seconds)`:
1. overall: the `overall` dict, or `{score: obj.overall}` when it is not a dict.
2. Scores: `scores` dict (else empty) → each of `sync`, `realism`, `mix_balance`, `loudness`, `music` through
   the score rule (C7.1).
3. Notes: `notes` dict (else empty) → for each of the 5 keys with a non-empty value, the trimmed string.
4. Timed lists (`issues` with keys `severity`, `category`, `problem`, `fix`; `synthetic_or_cheap` with keys
   `what`, `why`): the time rule of C7.1; other items that are not dicts are skipped; a non-list is empty.
5. Issue severity: lower-case; when it is not `high`, `medium` or `low`, it becomes `medium`.
6. Sort issues by severity (high, medium, low), then by start (no start = last).
7. `strengths`: non-empty strings, at most 8. `top_fixes`: non-empty strings, at most 5.
8. `answer`: the trimmed string, only when present and non-empty.

### C4.3 Result and summary

After `generate` returns:
- `ok` = the answer parsed.
- Not parsed: `raw` = the scrubbed answer, at most 20000 characters. Summary: the provider answered but not
  in parseable JSON, the finish reason, the raw answer is attached, plus the trim note.
- Parsed: `critique` = the normalized answer. Summary, 1 paragraph:
  `<provider> (<model>) rates the soundtrack <score>/10: <verdict> <n> issues (<h> high).` then
  ` Top fix: <first top fix>` when there is one, then a closing sentence: LLM opinion, timestamps
  approximate, verify with `hark timeline` / `ab`, plus the trim note.
  - score in the shortest number form, or the word `unscored` instead of `<score>/10` when null;
  - verdict = the overall verdict, or `no verdict`; a period is added when it does not end in `.`, `!` or `?`;
  - h = the number of high-severity issues.
- Trim note (when trimmed): a short bracketed note that only the first <seconds> s were sent and that `--start` / `--end` select other parts.
- `uploaded` = the clip info plus `method`, plus `files_api_deleted` when the provider gave it (not null).

### C4.4 Text (`render`)

Lines:
1. the summary;
2. `(note: <note>)` for each context note;
3. no critique: the raw answer (at most 3000 characters) when present, then stop;
4. `answer: <answer>` when present;
5. `scores (1-10, LLM opinion): <key> <value>, ...` for the non-null scores (shortest number form);
6. `  <key>: <note>` per note;
7. when issues exist: a heading line (most severe first; times are file seconds, approximate), then per issue
   `  <span> <severity> <category>: <problem> -> fix: <fix>`;
8. when synthetic or cheap items exist: a heading line, then per item `  <span> <what>: <why>`;
9. `strengths: a; b` when present;
10. `top fixes: 1. a | 2. b` when present.

Span: `(no time)` when start is null; `m:ss.ss` of start when end is null or equal to start; else
`m:ss.ss-m:ss.ss`. Time format: minutes, a colon, then seconds with 2 decimals padded to 5 characters
(`0:04.20`, `1:02.00`).

## C5. Source transport (reference only)

The source's hosted-API provider, given here so that the hark mapping keeps the same guarantees.

- Inline when the base64 text of the audio is at most 18 MiB (18 x 1024 x 1024 characters; the strictest
  documented inline limit is 20 MB). Else a resumable file upload: a start request returns an upload URL; 1
  upload-and-finalize request sends the bytes; poll the file state every 2 s, at most 60 times, while it is
  `PROCESSING`; it must end `ACTIVE` with a URI, else an error. The stored copy is deleted right after the
  answer, and also after a failure; a failure message says whether the copy was deleted (the vendor removes it
  after 48 h otherwise).
- Request: 1 user turn with 2 parts, the audio part and the prompt text. Generation settings: JSON response
  mime type and at most 16384 output tokens.
- Response: the first candidate; the text = the concatenated text parts that are not thought parts. No
  candidates: error with the block reason. Empty text: error with the finish reason. Also returned: finish
  reason, the model version, usage counts, method (`inline` or `files_api`), whether the stored copy was
  deleted (null when none was stored).
- Retries: on HTTP 429, 500, 502, 503, 504 and on network errors, wait 4 s, then 12 s (3 attempts). A timeout
  is not retried: error that the model did not answer in time, try a shorter clip. Network error after the last
  attempt: error naming the error type.
- Timeouts (connect, read): generate (20 s, 600 s); upload start (20 s, 120 s); upload (20 s, 900 s); state
  poll and delete (20 s, 60 s).
- A status below 400 passes. A higher status raises an error with the status, a hint and the vendor's error
  message. Hints by status: 400, a malformed request or a model without audio input; 401, the key was refused;
  403, the key cannot use this API or model; 404, unknown model id, so set the model variable or `--model`;
  413, the request is too big, so shorten it with `--max-seconds` or `--start` / `--end`; 429, rate or quota
  limit; any other status, a generic failure.
- A response that is not JSON: error that names the status and the first 120 characters (a proxy or
  firewall page).
- The key goes only in a request header. It is never printed, logged or stored.
- Per the vendor docs that the source cites, the model receives the audio reduced to 1 channel at 16 kbps (32
  tokens per second). High-frequency detail and stereo image are therefore lost to the model.

## C6. hark mapping to agy

### C6.1 Readiness

- Ready when `agy` is on PATH and the probe verdict in `docs/specs/hark/agy-audio-probe.md` is HEARS.
- When the verdict is DOES-NOT-HEAR, `hark llm` exits 2 with a clear message that agy cannot hear audio files,
  and sends nothing (ticket 21). The MCP tool returns the same message as a tool error.
- hark never reads, sets or passes an API key of any vendor. agy uses the operator's own sign-in.

### C6.2 Request

1. Run C1 to C3 unchanged. Consent stays required: agy also sends the audio off the machine.
2. Write the clip bytes to a temporary file with extension `.mp3` inside a fresh temporary folder.
3. Call agy with the invocation that the probe found best. The probe ticket tests this form:
   `agy -p "<prompt>" --add-dir <folder> --output-format json [--model <id>] --print-timeout <s>s`.
   The prompt names the clip file path, then holds the full prompt of C3.3.
4. Model: `--model` argument, else `HARK_LLM_MODEL`, else agy's default (no `--model` flag).
5. Timeout: 600 s for the call, as the source's generate timeout. No retries on a timeout. Retry once after 4 s
   on a non-zero exit that agy marks as transient (rate limit); else fail.
6. Delete the temporary folder after the call, also after a failure.

### C6.3 Response

`generate` returns:

| key | from agy |
|---|---|
| `text` | the answer text of the JSON output (the field that the probe names) |
| `finish_reason` | the finish or stop field when agy gives one, else null |
| `method` | `agy` |
| `files_api_deleted` | null |
| `model_version` | the model that agy reports, else the requested id, else `agy-default` |
| `usage` | token counts when agy gives them, else empty |

An empty answer is a provider error that names agy's exit code and the first 300 characters of its stderr.
Then C4 runs unchanged. The context text, rubric and schema do not change.

## C7. Algorithms and constants

### C7.1 Algorithms

Outermost object: find the first `{`. Walk forward and track string state: a `"` starts or ends a string; a
backslash escapes the next character inside a string (an escaped backslash does not escape the next one).
Outside strings, `{` adds 1 to the depth and `}` subtracts 1. When the depth is 0 again, return the text from
the first `{` to that `}`. When the text ends first, return the rest from the first `{`. No `{`: none.

Seconds rule: a number (not a boolean) is used as is. A string of the form `[m:]s[.fff][s]` with optional
spaces (for example `12.4`, `0:12.5`, `1:02`, `12.4s`) gives m x 60 + s. Anything else is none.

Score rule: a string gives its leading number (`"7/10"` gives 7), else none. A number (not a boolean) is
clamped to [0, 10] and rounded to 1 decimal. Else none.

Time rule per item: a = seconds(start), b = seconds(end). When a is known: a = clamp(a, 0, clip_len); b = a when
b is none, else clamp(b, a, clip_len). Output start = a + offset and end = b + offset, rounded to 2 decimals
(file seconds), or null. The named keys are copied as trimmed strings (missing = empty).

### C7.2 Constants

| name | value |
|---|---|
| clip codec | MP3, mono, 44100 Hz, 64 kbps |
| mime | `audio/mp3` |
| default max seconds | 600 |
| transcript cap | 16000 characters |
| cut list cap | 200 cuts |
| raw answer cap in result | 20000 characters; in text 3000 |
| inline base64 limit (source) | 18 MiB |
| max output tokens (source) | 16384 |
| retry statuses (source) | 429, 500, 502, 503, 504 |
| retry waits (source) | 4 s, 12 s |
| upload poll (source) | every 2 s, at most 60 times |
| severity order | high 0, medium 1, low 2 |
| score keys | `sync`, `realism`, `mix_balance`, `loudness`, `music` |
| issue categories asked for | `sync`, `realism`, `mix`, `loudness`, `music`, `noise`, `edit` |
| caps | strengths 8, top_fixes 5 |
| error text caps | ffmpeg stderr 300 characters |

Measured context keys, in this order. hark picks its own label words with the meaning given here:

| label | key | unit |
|---|---|---|
| integrated loudness | `lufs` | ` LUFS` |
| true peak | `true_peak_dbtp` | ` dBTP` |
| loudness range | `lra_lu` | ` LU` |
| short-term max | `short_term_max_lufs` | ` LUFS` |
| noise floor: the 10th percentile of 20 ms levels | `noise_floor_dbfs` | ` dBFS` |
| clipped samples | `clipped_samples` | (none) |

## C8. Errors and edge cases

| case | result |
|---|---|
| unknown provider | value error listing the providers |
| path not a file | audio error: no such file |
| provider not ready | off result, `ok` false, exit 1, nothing sent |
| no consent | off result naming the file and model, exit 1, nothing sent |
| agy verdict DOES-NOT-HEAR | exit 2 with a clear message, nothing sent |
| start >= end after clamping | failure result: empty range |
| clip longer than max seconds | sent trimmed; `trimmed` true; the summary says so |
| ffmpeg fails | failure result naming the file |
| timeline context fails | still sent without it; a note says why |
| cut detection fails | still sent without cuts; a note says why |
| model answer wrapped in a code fence or prose | parsed |
| trailing commas in the answer | parsed |
| answer not JSON at all | `ok` false; `raw` holds it; exit 1 |
| scores as strings such as `"8/10"` | 8.0 |
| score 12 | 10.0 |
| times as `"0:12.5"` | 12.5 plus the clip offset |
| issue time past the clip end | clamped to the clip end |
| unknown severity | `medium` |
| no `overall.score` | summary says `unscored` |
| any secret in an error | replaced by `***` |

## C9. Acceptance tests (LLM critique)

agy is faked in every test except 1 test marked `live`. No test sends audio off the machine unless marked
`live`.

| id | input | properties of the output |
|---|---|---|
| C-T1 | critique without consent, provider ready | `enabled` false, `ok` false; `how_to_enable` has 2 steps; the fake agy was not called; CLI exit 1 |
| C-T2 | agy not on PATH | off result; steps say to install and sign in; nothing sent |
| C-T3 | probe verdict DOES-NOT-HEAR, consent given | CLI exit 2; clear message; the fake agy was not called |
| C-T4 | `encode` of a 10 s WAV with start 2, end 5 | bytes are MP3; info `start` 2.0, `end` 5.0, `seconds` 3.0, `trimmed` false, `codec` `mp3 mono 64 kbps` |
| C-T5 | `encode` of a 10 s WAV with max_seconds 4 | `end` 4.0, `trimmed` true; the summary later holds the trim note |
| C-T6 | `encode` with start 8, end 3 | failure result whose message says empty range |
| C-T7 | `build_prompt(ctx, question="Is the hit late?", synth_intended=True)` | holds `Context:`, the context, the intent statement, the question; the schema holds `answer` |
| C-T8 | `build_prompt(ctx)` | the schema has no `answer` key; no intent statement |
| C-T9 | `parse_json` of a fenced answer with prose before it and a trailing comma | a dict |
| C-T10 | `parse_json("no json here")` | null |
| C-T11 | `normalize` of issues with severities `HIGH`, `odd`, `low` and starts 5, 1, null; offset 10, clip_len 20 | severities `high`, `medium`, `low` in that order; starts 15.0, 11.0, null |
| C-T12 | `normalize` with `"start": "0:30"` and clip_len 20, offset 0 | start 20.0 (clamped) |
| C-T13 | `normalize` with scores `{"sync": null, "realism": "8/10", "mix_balance": 12}` | sync null, realism 8.0, mix_balance 10.0, loudness null, music null |
| C-T14 | fake agy answer with a full valid JSON object, 3 issues (1 high) and top fixes | `ok` true; summary holds `/10`, `3 issues (1 high)`, `Top fix:`; text has the scores line and 3 issue lines with `m:ss.ss` spans |
| C-T15 | fake agy answer that is not JSON | `ok` false; `raw` set; CLI exit 1 |
| C-T16 | with a video that has cuts at 1.0, 4.0, 12.0 and a clip of 3 to 10 s, `with_timeline` false | context has a cut line with `1` (= 4.0 - 3.0) only |
| C-T17 | `with_timeline` true and a timeline that raises | the call proceeds; `notes` holds the NOT-sent note; the text shows `(note: ...)` |
| C-T18 | MCP `llm_critique` without consent | text result (not an error) that says how to enable; tool annotation `openWorldHint` true |
| C-T19 | any run | no environment variable named like an API key is read (the test checks access with a guard) |
| C-T20 | `live` (opt-in): a real agy call with consent on a 2 s click track | `ok` true or a clear failure result; never a traceback |
