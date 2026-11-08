# Listening through agy

agy (the Antigravity CLI) gives Gemini the real audio. Its file-reading tool
returns a WAV as a native `audio/wav` part, not as bytes or a header summary.
The probe of 2026-11-07 checked this with 4 WAV files of the same size and
header: Gemini named the class of all 4 correctly.

## What the ear gets right and wrong

| claim | reliable? |
|---|---|
| class: tone, noise, clicks, silence, wrong sound | yes, 4 of 4 |
| character: harsh, dull, synthetic, roomy | yes, use it |
| pitch | no: 1 kHz right, 440 Hz heard as about 700 Hz |
| hit count | no: wrong in 2 of 3 cases |
| onset and end times | no: about 0.2-0.5 s late, invents leading silence |
| MP3 input | no: an MP3 was heard as silence |

So the ear writes words and the meter (`hark measure`, `hark timeline`) writes
numbers.

## In a review session

Open the WAV with your file-reading tool, by its absolute path. Its folder
must be inside the workspace or added with `--add-dir`.

## Standalone invocation

Use this when a session cannot attach the audio itself, or for 1 extra blind
listen of 1 file in a clean context. This is the probe's best invocation:

```sh
env -u ANTHROPIC_API_KEY agy -p "<prompt that names the absolute path of a WAV file>" \
  --add-dir <folder that holds the file> \
  --model gemini-4-argon-high \
  --output-format json \
  --print-timeout 240s
```

- The prompt must not say what the file holds. List the options instead
  ("is it a tone, noise, clicks or silence?"), and tell the model not to run
  shell commands and to use only its hearing.
- Ask for qualitative judgement only: class, character, fit, defects.
- Keep the 240 s timeout. Runs take 55-107 s, and 1 of 6 probe runs hit 120 s.
- Accept the answer only when the JSON has `status` equal to `SUCCESS` and no
  `finished_reason` field. A `print_timeout` answer is not an answer: run it
  1 more time, then score that file from the meters only.
- `--input-format stream-json` cannot carry audio. Do not use it.
