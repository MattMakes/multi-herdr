# Brief: long-context accuracy evidence and compaction thresholds per model

Read `/Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/00-shared-context.md` FIRST.
Output: `/Users/mascott/projects/multi-herdr/ai_docs/reports/env-research/compaction-benchmarks.md`

## The question
The operator believes fleet workers should auto-compact EARLIER than the harness
default because long-context accuracy degrades well before the context window
is full, citing MRCR v2 and GraphWalks. Your job is to find the evidence per
configured model and per harness default, so the orchestrator can pick a
threshold per tier with a stated reason. You return findings with citations,
not a recommendation.

## Steps
1. Benchmarks. For each of these, find the published results (OpenAI's MRCR / MRCR v2 "multi-round co-reference", OpenAI GraphWalks, plus any of: Fiction.LiveBench, RULER, NoLiMa, LongBench v2, "context rot" studies such as Chroma's) and extract accuracy as a function of context length (the curve or the table, not just the headline):
   - Anthropic: Claude Opus 5, Claude Sonnet 5, Claude Fable 5.1 / Mythos 5.1 (whatever is published; if only Opus 4.x / Sonnet 4.x data exists, say so and report that as the nearest proxy).
   - OpenAI: gpt-5.6 (sol / terra variants if published separately; else the base gpt-5.6 or nearest gpt-5.x).
   - Nemotron 3.5 lightning, Nemotron 3 ultra (NVIDIA), "big-pickle" (find what model this actually is on the OpenCode free tier; https://opencode.ai/docs/ or the OpenCode zen/free-tier page).
   - Qwen3 8B (the `qwen3.8` Ollama tag): its stated context length and any long-context eval.
   Sources: model cards / system cards, https://openai.com/index/ posts for MRCR and GraphWalks, https://github.com/openai/mrcr , the Anthropic model pages https://docs.claude.com/en/docs/about-claude/models , NVIDIA model cards on Hugging Face, Qwen3 tech report / HF card, https://contextarena.ai (aggregates MRCR-style results per model) and https://fiction.live/stories/Fiction-liveBench . Cite URL per number.
   For each model give: context window; the context length at which accuracy first drops below ~90% of its short-context score; and the length at which it is below ~70%. If a source gives only a few points, tabulate them.
2. Harness defaults. For each harness report the default auto-compaction trigger and the knob that changes it (name it exactly and where it lives; the other researchers enumerate env vars, you only need the compaction knob and its default):
   - Claude Code 2.1.274: default autocompact percentage / token threshold; `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` and `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (verify existence via `strings /Users/mascott/.local/share/claude/versions/2.1.274 | grep -i compact`; the docs at https://docs.claude.com/en/docs/claude-code/settings and the changelog https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md). Also: does the 1M-context option change the window the percentage is taken from?
   - Codex CLI: the config key for auto compaction token limit and its default (https://github.com/openai/codex/blob/main/docs/config.md).
   - OpenCode: compaction/autocompact setting and default (https://opencode.ai/docs/config/).
   - pi / Prime: whether they compact at all, and the setting.
3. Cost of compacting. From the Claude Code docs and any Anthropic engineering posts on context management, note what a compaction costs (one summarization call over the full context; which tokens are cached vs not) and what it loses (working file state, in-flight tool results). Note the difference for a long-lived orchestrator (context precious, compaction destroys the "who owns what" map) versus a short-lived worker (a fresh worker is cheaper than a compacted one - the fleet already prefers fresh sessions).
4. Put a synthesis table at the top: model | window | ~90% point | ~70% point | harness default trigger | source quality (measured / proxy / unknown).

## Out of scope
No env-var enumeration beyond the compaction knobs. No file edits except the report. Do not recommend a threshold; report the evidence and let the orchestrator decide.

## Done looks like
Report with the synthesis table, per-model evidence with URLs, per-harness default table, the cost-of-compaction section. Then `horch done` with the path.
