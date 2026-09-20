# Brief: pi + Ollama (qwen3.8) and Prime Agent env vars / config for fleet efficiency

Read `/Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/00-shared-context.md` FIRST.
Output: `/Users/mascott/projects/multi-herdr/ai_docs/reports/env-research/pi-ollama-prime.md`
(three sections: pi, Ollama, Prime Agent; a top-10 for pi+Ollama combined and a top-10 for Prime).

## Part A: pi (`pi`, model `ollama/qwen3.8`)
1. `pi --version | head -c 200`. Find install (`readlink -f $(which pi)`); it is a JS bundle. `pi --help | head -80`.
2. Enumerate `strings <bundle> | grep -oE '(PI_|OLLAMA_|OPENAI_|ANTHROPIC_)[A-Z0-9_]+' | sort -u`.
3. Docs: the pi project README / docs (find the repo from `pi --help` or the package.json next to the bundle: `npm ls -g pi 2>/dev/null` or look for `node_modules/@*/pi*`). Look for: settings file location and keys, `--thinking` levels and what they map to for an Ollama model, context-window handling and compaction (does pi compact? at what threshold? key name?), tool output truncation limits, session storage, `--no-extensions/--no-skills` (already set by horch), any env vars.
4. Note flags horch already passes (see shared context) and `crates/horch-core/src/launch.rs` `pi_family_command`.

## Part B: Ollama (server behind pi, model qwen3.8)
1. `ollama --version | head -1`, `ollama show qwen3.8` (record the model's context length, parameter size, quantization, template/thinking support), `ollama ps`.
2. Official env var reference: https://github.com/ollama/ollama/blob/main/docs/faq.md and https://docs.ollama.com/ (search for `OLLAMA_CONTEXT_LENGTH`, `OLLAMA_NUM_PARALLEL`, `OLLAMA_MAX_LOADED_MODELS`, `OLLAMA_KEEP_ALIVE`, `OLLAMA_FLASH_ATTENTION`, `OLLAMA_KV_CACHE_TYPE`, `OLLAMA_NUM_GPU`/`num_gpu`, `OLLAMA_HOST`, `num_ctx`, `num_predict`, `think`). Also `ollama serve --help` and `strings $(readlink -f $(which ollama)) | grep -oE 'OLLAMA_[A-Z0-9_]+' | sort -u`.
3. THE key accuracy trap: Ollama's default context window (`num_ctx` / `OLLAMA_CONTEXT_LENGTH`) is small and truncates the prompt SILENTLY. Report the default in the installed version, qwen3.8's max, and how pi sets `num_ctx` (does it pass options per request? check the pi source for `num_ctx`). A worker whose system prompt plus skill catalog exceeds num_ctx silently loses its instructions. This is an accuracy fix first, an efficiency lever second.
4. Report the machine's RAM/VRAM (`sysctl hw.memsize`, `system_profiler SPDisplaysDataType | head -20`) so the recommended `num_ctx` is realistic.
5. Quality-affecting knobs (`OLLAMA_KV_CACHE_TYPE` q8_0/q4_0, `OLLAMA_FLASH_ATTENTION`) go in TUNE CAREFULLY or SKIP with reasoning, not RECOMMEND. Qwen3 thinking mode on/off: report how it is toggled and the tradeoff.

## Part C: Prime Agent (`prime`, model `anthropic/claude-opus-5`)
1. `prime --version | head -c 200` (it may dump a bundle; cap it). `prime --help | head -80`. Find the bundle and grep `(PRIME_|PI_|ANTHROPIC_)[A-Z0-9_]+`.
2. Prime is a pi fork with a daemon and a single persistent Python kernel tool, and horch launches it with `--session-dir` (see `launch.rs` `pi_family_command` and the comments about Prime 0.9.4). Its model calls go through the Anthropic API, so the levers are: `--thinking` levels (which Anthropic thinking budget each maps to), prompt caching (is it on? verify in source: look for `cache_control`), max output tokens, kernel output truncation (how much stdout from the Python kernel is fed back per call - this is the big token sink), session-dir contents, daemon env vars, any compaction.
3. Fill every field from the shared context.

## Out of scope
No Claude Code, Codex, OpenCode, no benchmark papers. Do not change any Ollama settings or pull models. Do not edit any file except the report.

## Done looks like
Report with version headers (pi, ollama, prime), `ollama show` summary, hardware line,
enumeration counts, two top-10 tables with all fields, unverified lists, skip lists.
Then `horch done` with the path.
