# Setup, surfaces and the optional LLM critique

Everything runs on the local CPU. The one path that sends audio off the machine is the optional LLM critique below.
hark caches embeddings and results on disk under `HARK_HOME` (default `~/.cache/hark`).

## The launcher and how a project tool finds it

- The launcher is `scripts/hark` inside the hark skill folder: `skills/hark/scripts/hark` in this repo, and the same
  relative path when a fleet loads the skill from a skill bundle. Call it by its path; do not assume `hark` is on
  PATH.
- It runs the Python named by `HARK_PYTHON` (default `~/.local/state/hark/venv/bin/python`) with the engine folder
  `skills/hark/engine` on the import path. When it finds no venv it does not create one: it exits 3 with a message.
- A tool in another skill resolves the launcher as a sibling: `<its skill folder>/../hark/scripts/hark`. The
  `sound-sourcing` scripts do this (`{{HARK}}` in their briefs).
- A project script that needs the Python API runs with that same Python, puts `<hark skill folder>/engine` on
  `sys.path` (or `PYTHONPATH`), then `import hark`. Never copy the engine into a project.
- Check the install with `skills/hark/scripts/hark doctor`. It reports packages, ffmpeg and the models (pinned
  revision, cached or not). `--load` also times 1 model load. `--fetch` downloads the models up front.

## Surfaces and their cost

| surface | cost | use when |
|---|---|---|
| CLI: `hark <command> ... [--json] [--out result.json]` | a fresh process per call; model loading first, about 20-40 s on a loaded laptop | a few calls |
| MCP server: `hark mcp` (models stay loaded) | a few seconds per call after the first | many calls in 1 session |
| 1 Python script: `import hark; hark.find(...); hark.realness(...)` | 1 model load for the whole script | batch work |

CLI conventions: text output opens with a plain-language summary. Times are `m:ss.ss` in text and seconds in JSON.
Errors are 1 stderr line, `hark <command>: error: <message>`, exit 1. Exit 2 is a bad command line or a command not
built yet. Exit 3 is an install that was needed and not done. Input is anything ffmpeg decodes, including the audio of
an mp4. A video without audio produces a message that says so.

The first model call of a process pays the CLAP and tagger load (about 20-60 s). On a machine shared with other work,
limit threads: `HARK_THREADS=2` (or 3), or `--threads 2` on any command.

## The optional ML install

CLAP and the AudioSet tagger (torch and transformers) are an optional extra. The model-free commands (`measure`,
`sfx`, `kit`, `gallery`, `forge --render`) never need it. When an ML command finds the extra missing:

- on a terminal, hark prints the missing packages and the install command, asks `Install them now? [y/N]`, installs on
  yes and runs the command again;
- without a terminal (an agent, a pipe), hark prints 1 line,
  `hark <command>: error: MLMissing: <feature> needs the ml extras. Install them with: <command>`, and exits 3.

As an agent, show that command to the user and ask before you run it: it downloads torch (CPU wheel) and
transformers into the hark venv. Then run `hark doctor --fetch` to download the pinned models.

## MCP

Register the server through the launcher with `HARK_THREADS=3`:

```json
{"mcpServers": {"hark": {"command": "<path to skills/hark/scripts/hark>", "args": ["mcp"], "env": {"HARK_THREADS": "3"}}}}
```

Tool arguments take absolute paths. `full=true` returns a tool's complete JSON. When a client lists no hark tools,
restart it with `MCP_TIMEOUT=60000`. `hark mcp --warm` loads the models at start. Tools: measure, describe, match,
rank, find, compare, ab, realness, timeline, sheet, critique, doctor, llm_critique, and 3 model-free bank tools:
sfx_list, sfx_render, kit_render. The server needs the `mcp` extra; it never prompts. Project files never hold a
literal key or secret.

## Optional second opinion in words (LLM critique)

hark can send a short audio clip to an audio-capable model through the Antigravity CLI (`agy -p`, the user's own
sign-in; no API key). It returns a rubric critique: sync, realism, mix, loudness, what sounds cheap, and timed fixes.
The probe found that agy hears coarsely: it names the sound class and gross faults, but its times are off by 0.2-0.3 s
and its counts are wrong. hark therefore sends its own measured times as context.

Rules:

- Ask first. The critique runs only after the user explicitly agrees that the audio leaves the machine. Never set the
  consent flag for the user.
- Then, with that agreement only: `hark llm <file> --yes-upload [--with-timeline] [--prompt "<question>"]` (MCP:
  `llm_critique` with `consent=true`).
- The upload is audio only, mono, a short clip. Video frames are never sent.
- `--with-timeline` attaches hark's listening transcript; `--prompt` adds 1 specific question.
- Its times are rough and its scores are 1 model's view. Confirm every suggested fix with `hark timeline` and
  `hark ab` before you act on it.
