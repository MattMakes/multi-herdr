# Probe: can `agy -p` hear an audio file?

Date: 2026-11-07. Worker: opus-40 (ticket T-11). agy build: the one at `~/.local/bin/agy` on this date.

## Verdict: HEARS (coarse only)

agy delivers the real audio to Gemini as a multimodal part. Gemini then names the sound class correctly for 4 of 4 WAV files (tone, noise, clicks, tone after silence). It also names the 1 kHz pitch correctly. Measurements are not reliable: it gets the 440 Hz pitch wrong, gets hit counts wrong in 2 of 3 cases, and invents about 0.4 s of leading silence in 3 of 4 WAV files. It heard the MP3 copy as silence.

Consequence for hark: an `audio-reviewer` on agy can give qualitative critique (what kind of sound, rough character, a gross error such as a silent file or the wrong sound). Every number (pitch, onset, count, duration, loudness) must come from hark's own DSP, not from agy. Send WAV, not MP3.

## Test files

Made with ffmpeg in `ai_docs/probe-audio/` of the worktree (git-ignored scratch). The names are neutral so that the name does not give the content away.

| File | Content | ffmpeg source |
|---|---|---|
| `a.wav` | 440 Hz sine, 2.0 s, starts at 0 s | `-f lavfi -i "sine=frequency=440:duration=2" -ar 44100` |
| `b.wav` | white noise, 2.0 s, starts at 0 s | `-f lavfi -i "anoisesrc=color=white:duration=2:amplitude=0.5" -ar 44100` |
| `c.wav` | 3 clicks at 120 BPM (0.0, 0.5, 1.0 s), 1.5 s total. Each click is a 30 ms 2 kHz burst with fast decay | `-f lavfi -i "aevalsrc='if(lt(mod(t,0.5),0.03),sin(2*PI*2000*t)*exp(-mod(t,0.5)*80),0)':d=1.5:s=44100"` |
| `d.wav` | 1.0 s silence, then 1 kHz tone for 1.0 s | `-f lavfi -i "aevalsrc='if(gte(t,1),sin(2*PI*1000*t)*0.5,0)':d=2:s=44100"` |
| `c.mp3` | MP3 copy of `c.wav`, 128 kb/s, 1.54 s | `-i c.wav -codec:a libmp3lame -b:a 128k` |

I checked the files with ffprobe, volumedetect and silencedetect. All files are correct. Peak levels: a -18 dB, b -2 dB, c -0.2 dB, d -6 dB, c.mp3 -0.5 dB. silencedetect on `c.wav` finds sound at 0.00, 0.50 and 1.00 s. On `d.wav` it finds silence from 0.0 to 1.0 s.

## Models (`agy models`)

```
gemini-4-argon-medium / -high / -low
gemini-3.8-flash-high / -medium / -low
claude-opus-5-5-low / -medium / -high
claude-sonnet-5-5-low / -medium / -high
gpt-oss-120b-medium
```

The best Gemini model is `gemini-4-argon-high`. All runs used it.

## Command

```sh
env -u ANTHROPIC_API_KEY agy -p "Open the audio file <abs path> and listen to it. \
Do not run shell commands or scripts; use only your own hearing of the audio. \
Describe what you hear: is it a tone, noise, clicks or silence? If a tone, what pitch in Hz? \
How many distinct hits? At what time does sound start and stop? Also say how you got the \
audio content (native audio input, or reading bytes/metadata)." \
  --add-dir <folder> --model gemini-4-argon-high --output-format json --print-timeout 120s
```

The prompt does not say what the file holds. The prompt lists all 4 classes for every file.

## Answers and grades

Grades: correct, partly correct (class right, a measurement wrong), wrong, cannot hear.

| File | Truth | agy answer (short) | Grade |
|---|---|---|---|
| `a.wav` | steady 440 Hz, 0.0-2.0 s, 1 sound | pure tone, ~700 Hz (650-750), 3 beeps, starts ~0.45 s, stops ~1.5 s | partly correct: class right; pitch, count, timing wrong |
| `b.wav` | white noise, 0.0-2.0 s, 1 sound | broadband white noise, unpitched, 1 burst, starts ~0.45 s, stops ~1.6 s | partly correct: class, pitch and count right; timing wrong |
| `c.wav` run 1 | 3 clicks | no answer: print timeout after 120 s (`finished_reason: print_timeout`) | not graded |
| `c.wav` run 2 (`--print-timeout 240s`) | 3 clicks at 0.0/0.5/1.0 s, 1.5 s total | clicks, unpitched, 5 hits ~0.35 s apart, first at ~0.45 s, clip ~2.0-2.5 s | partly correct: class right; count, spacing, timing, length wrong |
| `d.wav` | silence 0-1 s, 1 kHz tone 1-2 s | pure tone, ~1000 Hz, 1 hit, starts ~1.2 s, stops ~1.7 s | partly correct, best answer: class, pitch, count right; start 0.2 s late; stop 0.3 s early |
| `c.mp3` | 3 clicks | complete silence, 0 hits, clip 2-3 s | wrong |

Every answer says the same thing about the input: "`read_file` returned `Showing binary file of type audio/wav` with an attached multimodal `audio/wav` part (`part_0`)". For the MP3 it says `audio/mpeg`.

## Evidence of real audio input

1. The JSON output (`--output-format json`) has only `response`, `status`, `duration_seconds`, `num_turns` and `usage`. It has no tool trace. `input_tokens` is 9651-9695 for every file, so token counts do not show the audio part.
2. The local conversation store (`~/.gemini/antigravity-cli/conversations/<conversation_id>.db`, table `steps`) shows the turn. Each conversation has exactly 4 steps: user input, planner, 1 tool step, final answer. There is no shell command step.
3. The tool step payload size follows the file size. `a.wav` is 176478 B and its step is 178658 B. `c.wav` is 132378 B and its step is 134589 B. `c.mp3` is 25120 B and its step is 27338 B. So the whole file goes into the turn, labelled `audio/wav` or `audio/mpeg`. The model does not get a hex dump or a header summary.
4. The classes differ by file and match the content. `a`, `b` and `d` have the same size (176478 B) and byte-identical 78 B headers (RIFF, fmt, LIST, data), so metadata alone cannot give tone vs noise. The model therefore gets content from the audio.

Conclusion: agy attaches audio natively through its `read_file` tool. The model hears coarse content. The time and count errors come from the model, not from the transport.

## `--input-format stream-json`

It cannot attach audio.

- `-p` takes the next argument as its prompt. Use `-p=""` before `--input-format stream-json --output-format stream-json`.
- Each stdin line must be `{"event":"user","message":{...}}`. A line without `event` fails with `stream input message is missing the "event" field`.
- A content block `{"type":"audio","source":{"type":"base64",...}}` fails at once with `stream input content block type "audio" is not supported (only "text")`. No turn runs and no tokens are used.

So stream-json carries text only. A stream-json text message that names a file path uses the same `read_file` path as `-p`.

## Best invocation

```sh
env -u ANTHROPIC_API_KEY agy -p "<prompt that names the absolute path of a WAV file>" \
  --add-dir <folder that holds the file> \
  --model gemini-4-argon-high \
  --output-format json \
  --print-timeout 240s
```

- Use WAV. The MP3 copy was heard as silence.
- Use a timeout of 240 s. Runs took 55-107 s, and 1 of 6 runs hit the 120 s limit.
- Check `status == "SUCCESS"` and the absence of `finished_reason` in the JSON.
- Ask for qualitative judgement only. Measure every number with hark.

## Call count

The ticket allows 10 agy calls. I used 6 model turns (`a`, `b`, `c` twice, `d`, `c.mp3`). I also made 4 stream-json invocations that failed at argument or message validation with 0 turns and 0 tokens. `agy models` ran once. I did not repeat any file for a consistency check, so this probe does not measure how much the answers change between runs.
