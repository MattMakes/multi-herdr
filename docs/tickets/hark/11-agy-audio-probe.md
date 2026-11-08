# 11: Probe: can agy -p hear an audio file?

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/hark/agy-audio-probe.md`
**Own:** `docs/specs/hark/agy-audio-probe.md`

## Goal

A measured yes or no: can the Antigravity CLI (`agy -p`, Gemini) hear the content of a local audio file and describe it correctly?

**Side: clean.** You do not need the source.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Make 4 test files with ffmpeg in a folder under your worktree, not under /tmp: a 440 Hz sine of 2 s; white noise of 2 s; a 3-hit click track at 120 BPM; 1 s silence then a 1 kHz tone of 1 s. Also make 1 mp3 copy of the click track.
2. Run `agy models` and record the models.
3. For each file, run `agy -p "<prompt>" --add-dir <folder> --output-format json` with a prompt that names the file path and asks what is heard (pitch, noise or tone, count of hits, where the sound starts). Do not tell agy what the file holds. Try with `--model` set to the best Gemini model in the list. Use `--print-timeout 120s`.
4. Grade each answer: correct, partly correct, wrong, or "cannot hear" (agy reads bytes or metadata only). Look at the json output for signs of real audio input versus a file-read of bytes.
5. Also test the `--input-format stream-json` path if it can attach a file, and record the result.
6. Write `docs/specs/hark/agy-audio-probe.md`: the commands, the raw answers (short), the grades, the verdict HEARS or DOES-NOT-HEAR, and the exact invocation that works best.

## Constraints

- Never set or use ANTHROPIC_API_KEY. Do not add a Gemini API key. agy uses the operator's sign-in.
- Spend at most 10 agy calls.

## Done when

- The probe file holds a verdict with evidence. Report the verdict in your DONE line.

