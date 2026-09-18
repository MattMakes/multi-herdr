# Long-context accuracy vs. compaction thresholds, per fleet model and harness

Researcher: researcher-2 · Date: 2026-09-18 · Brief: `ai_docs/plans/env-research/04-compaction-benchmarks.md`

Installed versions (checked 2026-09-18):

| harness | binary / package | version |
|---|---|---|
| Claude Code | `/Users/mascott/.local/share/claude/versions/2.1.274` | 2.1.274 |
| Codex CLI | `/opt/homebrew/Caskroom/codex/0.154.0/bin/codex` | codex-cli 0.154.0 (source checked at git tag `rust-v0.154.0`, commit `6b9826e3`) |
| OpenCode | `/Users/mascott/.opencode/bin/opencode` | 1.18.2 |
| pi | `@earendil-works/pi-coding-agent` (`/opt/homebrew/bin/pi`) | 0.85.1 |
| Prime Agent | `prime-agent` npm package (`/opt/homebrew/bin/prime-agent`; no `prime` on PATH, and none is needed: horch launches `prime-agent`, overridable by `HORCH_PRIME_BIN`, `crates/horch-core/src/agent.rs::prime_bin`) | 0.9.4 |
| Ollama | `ollama` | 0.32.15 |

This report gives evidence only. It does not recommend a threshold (per the brief).

---

## 0. Synthesis table

"90% / 70% point" = the context-length bin where the score first drops below 90% / 70% of the same model's shortest-length score on the same benchmark (section 1 defines it). **CA** = Context Arena, third-party OpenAI-MRCR v2 8-needle, per-bin, run at the effort shown (1.0a). Bins are ranges, e.g. "128K–256K" means the drop is first seen for prompts of 128K–256K tokens. Trigger = the harness's default auto-compaction point for this model (section 2), in tokens and as % of the window the harness uses.

| model (harness) | window | ~90% point | ~70% point | harness default trigger | source quality |
|---|---|---|---|---|---|
| claude-opus-5 (Claude Code `opus`; Prime) | 1,000,000 | **128K–256K** (CA, max effort: .913 → .656) | **128K–256K** (same bin) | Claude Code **967,000** (96.7%); Prime **983,616** (98.4%) | measured, third-party, max effort only. Anthropic publishes no curve for 5.x |
| claude-sonnet-5 (Claude Code `sonnet`) | 1,000,000 | **32K–64K** (CA, max: .879 → .680) | **64K–128K** | **967,000** (96.7%) | measured, third-party, max effort only |
| claude-fable-5-1 (orchestrator) | 1,000,000 | unknown. Cross-model proxies: Opus 5 (above); "Claude Mythos 5" GraphWalks BFS 91.1% @256k → 79.4% @1M (first-party, OpenAI's table) | unknown | **967,000** (96.7%) | **unknown** (proxy only) |
| gpt-5.6-sol (Codex) | 272,000 (max 872,000) | **128K–256K** (CA, max: .924 → .835). OpenAI's own: below 90% only in 512K–1M, relative to a 256K–512K baseline | **256K–512K** (CA). OpenAI: not reached ≤1M | **244,800** (90%) | measured: third-party full curve + first-party (≥256K only) |
| gpt-5.6-terra (Codex) | 272,000 (max 872,000) | **64K–128K** (CA, max: .897 → .802) | **128K–256K** (CA .679) | **244,800** (90%) | measured: third-party full curve + first-party (≥256K only) |
| nemotron-3.5-lightning (OpenCode) | 262,144 declared (1M native) | not measured below 256K. RULER 256K → 1M keeps 90.6%. Proxy (Nemotron 3 Nano base RULER): 128K–256K | not reached (RULER ≤1M) | **230,144** (87.8%) | measured, first-party, 2 points ≥256K; proxy below |
| nemotron-3-ultra (OpenCode) | 1,000,000 | **256K–512K** (RULER, *base* checkpoint 95.3 → 84.5). Served checkpoint: single point 94.7 @1M | not reached ≤1M (base RULER 80.6% at 1M) | **968,000** (96.8%) | measured, first-party RULER; no MRCR/GraphWalks |
| big-pickle (OpenCode) | 200,000 (input 160,000) | unknown | unknown | **140,000** (70% of context) | **unknown**: underlying model undisclosed |
| qwen3.8 = Qwen3.8-27B (pi, Ollama Q4_K_M) | 262,144 | **64K–128K** (CA low/medium, .93–.98 → .79–.82); 32K–64K at xhigh | not reached ≤128K; nothing measured >128K | **245,760** (93.75%) | measured, third-party, full precision, ≤128K only |

Observations that come straight from the table (evidence, not a recommendation):
1. For every fleet model that has a per-bin curve, the harness's default trigger sits **at or beyond** the measured 90% point. On 1M windows the gap is large: Opus 5 and Sonnet 5 cross 90% somewhere between 32K and 256K, but Claude Code compacts at 967K. Codex's 244,800 falls **inside** Sol's 90% bin (128K–256K) and inside Terra's 70% bin (the same range). Qwen3.8-27B's trigger (245,760) is beyond anything measured for it.
2. The two sources disagree on level. Vendor self-reports at long lengths run 20–30 points above Context Arena for the same bin (Sol 256K–512K: 91.5% OpenAI vs .619 CA; Opus 4.6 128K–256K: 93.0 Anthropic vs .69–.72 CA). Both show the same shape: flat, then a decline starting around 64K–256K.
3. MRCR 8-needle and GraphWalks are the hardest retrieval tests. RULER and agentic-coding measures look much gentler on the same models (Nemotron 3 Ultra served RULER 94.7 @1M; Anthropic's ProgramBench, episodes "up to the full 1M token window", Opus 5 83% → 93%). How MRCR-style decline translates into coding-agent quality is not measured by any source found.
4. Anthropic's own practice for these 1M models is split: its BrowseComp evals compact at **200k**, its API compaction defaults to **150k** (min 50k) with the docs' reason "As a conversation grows, response quality degrades", while Claude Code's `auto` window compacts at **967k**, described as "tuned for your model … for the best cost and performance".
5. The knob that exists differs per harness: Claude Code has a lower-only percentage (`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`) and an absolute window (`CLAUDE_CODE_AUTO_COMPACT_WINDOW`, 100k–1M → trigger = value − 33k); Codex takes an absolute lower-only token limit; OpenCode's `compaction.reserved` has no effect on the Nemotron models as shipped (a `limit.context` or `limit.input` override moves them); pi/Prime take `compaction.reserveTokens`.

---

## 1. Benchmarks: accuracy vs. context length, per model

Definitions used throughout. **90% point** = the shortest tested length at which the score first falls below 90% of that model's score at the shortest tested length *on the same benchmark curve*; **70% point** likewise. When points are sparse the answer is a bracket `(last above, first below]`. "Not reached" means not reached within the tested range, which is not the same as "no degradation". Every number has a URL. Numbers marked ✔ were re-checked by me directly against the source page; the rest come from sub-agent research with the URL given.

### 1.0 What MRCR v2 and GraphWalks measure (the operator's cited benchmarks)

- **OpenAI-MRCR (multi-round co-reference).** A long synthetic multi-turn conversation contains several identical requests (e.g. "write a poem about tapirs"); the model must reproduce the *i*-th one. 2-, 4- or 8-needle variants; scored by string-match ratio. Bins by prompt+answer tokens from 4K–8K up to 512K–1M. v2 (2025-12-05) fixes mis-generated items. Introduced in https://openai.com/index/gpt-4-1/ ; data and scoring code: https://huggingface.co/datasets/openai/mrcr , https://github.com/openai/mrcr .
- **GraphWalks.** The context is filled with a random directed graph of hex-hash nodes; the model must return the nodes at depth *d* by breadth-first search (**BFS**) or all **parents** of a node. Scored by set F1. https://openai.com/index/gpt-4-1/ , https://huggingface.co/datasets/openai/graphwalks .
- Both are closer to "track many similar items across the whole context and reason over them" than to single-needle retrieval, which is why they fall off much earlier than RULER/needle tests for the same model.

### 1.0a Context Arena (third-party MRCR v2 8-needle, per bin) — the one source that covers most fleet models

`https://contextarena.ai/api/needle-summary?needles=8` returns the leaderboard as JSON (184 model × effort rows; fetched 2026-09-18 with curl; rows show `api_updated_at 2026-08-26`). Bins are OpenAI-MRCR's (the column label is the bin's upper edge: "128K" = 64K–128K tokens, "256K" = 128K–256K, etc.). Score = mean over n tests (n = 80 / 80 / 136 / 85 / 103 / 141 / 236 per bin). All rows below ✔ (extracted by me from the JSON). `–` = not tested.

| model (effort tested) | 8K | 16K | 32K | 64K | 128K | 256K | 512K | 90% point (bin) | 70% point (bin) |
|---|---|---|---|---|---|---|---|---|---|
| claude-opus-5 (max) | .987 | .999 | .993 | .999 | .913 | **.656** | .425 | 128K–256K | 128K–256K |
| claude-sonnet-5 (max) | .964 | .949 | .879 | **.680** | .529 | .522 | .320 | 32K–64K | 64K–128K |
| claude-opus-4.8 (max) (proxy) | .974 | .961 | .930 | .907 | .752 | .618 | .398 | 64K–128K | 128K–256K |
| gpt-5.6-sol (max) | 1.000 | 1.000 | .980 | .988 | .924 | .835 | .619 | 128K–256K | 256K–512K |
| gpt-5.6-terra (max) | .988 | 1.000 | .943 | .897 | .802 | .679 | .498 | 64K–128K | 128K–256K |
| qwen3.8-27b (low) | .993 | .956 | .953 | .933 | .818 | – | – | 64K–128K | not reached ≤128K |
| qwen3.8-27b (medium) | .993 | .992 | .967 | .979 | .789 | – | – | 64K–128K | not reached ≤128K |
| qwen3.8-27b (xhigh) | 1.000 | .990 | .929 | .874 | .743 | – | – | 32K–64K | not reached ≤128K |
| qwen3.8-27b (no thinking) | .603 | .441 | .515 | .429 | .347 | – | – | (low base; see 1.3) | |
| nemotron-3-super-120b (enabled) (proxy) | .541 | .447 | .603 | .507 | .381 | – | – | (low base; see 1.5) | |

Caveats that matter for reading this table:
- **Effort.** Claude 5 and GPT-5.6 rows exist only at `max`. The fleet runs different efforts (`horch teammates --json`, 2026-09-18): every Claude teammate (opus, sonnet, fable orchestrator) `xhigh`; codex-sol / codex-terra unset (Codex default); pi and prime `high`; opencode-lightning `minimal`, opencode-pickle / -ultra `high`. Where Context Arena has several efforts for one model, effort does **not** move long-length scores monotonically: at 64K–128K Qwen3.8-27B scores .818 (low) > .789 (medium) > .743 (xhigh); at 256K–512K GPT-5.5 scores .576 (medium) vs .542 (xhigh); at 128K–256K Opus 4.6 scores .718 (medium) vs .691 (high). Turning reasoning *off* does drop scores sharply from the first bin (Qwen3.8-27B .603, Opus 4.6 .816, GPT-5.5 .664 at 4K–8K).
- **Provider.** Claude rows were run via "Claude Platform on AWS", Qwen3.8-27B via "AkashML" (full precision, not the fleet's local Q4_K_M), Nemotron 3 Super via DeepInfra (`run_provider_names`).
- **Third-party vs. vendor numbers disagree by 20–30 points at long lengths.** Same benchmark and bin: GPT-5.6 Sol 256K–512K is 91.5% per OpenAI (1.1) vs .619 here; Claude Opus 4.6 128K–256K is 93.0 (max) per Anthropic's Sonnet 4.6 system card (1.2) vs .691 (high) / .718 (medium) here. The cause was not resolved (effort, harness, API limits — Anthropic notes some of its MRCR problems "exceed [the public API's] 1M token limit" — or scoring). Both show the same *shape*: flat to ~64K–128K, then a decline.
- Qwen3.8-27B was only tested to the 64K–128K bin even though its row lists `max_context_length 1000000`.

### 1.1 OpenAI gpt-5.6-sol / gpt-5.6-terra (Codex)

GPT-5.6 launch post, "Long context" table (https://openai.com/index/gpt-5-6/, all ✔ via the r.jina.ai text rendering of that page; openai.com returns 403 to direct fetch). OpenAI publishes only two bins for 5.6, both above Codex's 272,000 default window:

| eval | Sol | Terra | (Luna) | (GPT-5.5) |
|---|---|---|---|---|
| MRCR v2 8-needle 256K–512K | 91.5% | 89.6% | 41.3% | 81.5% |
| MRCR v2 8-needle 512K–1M | 73.8% | 72.5% | 41.3% | 74% |
| GraphWalks BFS 256k F1 | 90.7% | 76.9% | 81.3% | 73.7% |
| GraphWalks BFS 1M F1 | 77.1% | 71.2% | 51.2% | 45.4% |

Same table, Claude columns (useful in 1.2): GraphWalks BFS 256k / 1M — Claude Mythos 5 91.1% / 79.4%; Claude Mythos Preview 85.7% / 74.3%; Claude Opus 4.8 85.9% / 68.1%.

- Sol: MRCR 512K–1M is 80.7% of the 256K–512K value → 90% point in **(256K–512K, 512K–1M]**; 70% not reached by 1M. GraphWalks BFS 1M is 85.0% of 256k → 90% point in **(256k, 1M]**.
- Terra: MRCR 80.9% → 90% point in (256K–512K, 512K–1M]; GraphWalks BFS 1M is 92.6% of 256k → not reached.
- These are relative to a 256K baseline. **Nothing is published for GPT-5.6 below 256K**, where Codex's default 272k window (trigger 244.8k) sits.

PROXY for the shape below 256K — GPT-5.5, full MRCR v2 8-needle curve (https://openai.com/index/introducing-gpt-5-5/ ✔; footnote: "Evals of GPT were run with reasoning effort set to xhigh and were conducted in a research environment"):

| bin | 4K–8K | 8K–16K | 16K–32K | 32K–64K | 64K–128K | 128K–256K | 256K–512K | 512K–1M |
|---|---|---|---|---|---|---|---|---|
| GPT-5.5 | 98.1% | 93.0% | 96.5% | 90.0% | 83.1% | 87.5% | 81.5% | 74.0% |
| GPT-5.4 | 97.3% | 91.4% | 97.2% | 90.5% | 86.0% | 79.3% | 57.5% | 36.6% |
| Claude Opus 4.7 | – | – | – | – | – | 59.2% | – | 32.2% |

Same page, GraphWalks F1 256k / 1M: GPT-5.5 BFS 73.7 / 45.4, parents 90.1 / 58.5; GPT-5.4 BFS 62.5 / 9.4, parents 82.8 / 44.4; Claude Opus 4.7 BFS 76.9 / 41.2 ("Opus 4.6" at 1M), parents 93.6 / 72.0 ("Opus 4.6" at 1M).

- GPT-5.5 MRCR: 90% point **64K–128K** (84.7% of the 4K–8K score; non-monotonic, 128K–256K recovers to 89.2%); 70% point **not reached** by 1M (75.4%).
- GPT-5.4 MRCR: 90% point **64K–128K** (88.4%); 70% point **256K–512K** (59.1%).
- Second PROXY, GPT-5.2 Thinking (https://openai.com/index/introducing-gpt-5-2/, sub-agent via r.jina.ai, not re-checked by me): 98.2 → 89.3 → 95.3 → 92.0 → 85.6 (64K–128K) → 77.0 (128K–256K): 90% point 64K–128K; 70% not reached by 256K.
- Other trackers: RULER, NoLiMa, Fiction.LiveBench (changelog last updated 2026-04-04, before GPT-5.6's 2026-07-09 GA), and contextarena.ai (JS-only, not retrievable) had no GPT-5.6 rows. The GPT-5.6 system card (https://deploymentsafety.openai.com/gpt-5-6/gpt-5-6.pdf) has no long-context section.
- Third-party full curve for Sol and Terra themselves (max effort) is in 1.0a: Sol 90% point 128K–256K, 70% point 256K–512K; Terra 90% point 64K–128K, 70% point 128K–256K. Codex's default trigger (244,800) sits in the 128K–256K bin, where Context Arena measures Sol at .835 and Terra at .679.
- Source quality: **measured, first-party, sparse** (Sol/Terra, ≥256K only); **measured, third-party, full curve** (Context Arena, max effort); proxy (GPT-5.5/5.4/5.2) for vendor-side shape <256K.

### 1.2 Anthropic Claude Opus 5, Sonnet 5, Fable 5.1 (Claude Code; Prime runs Opus 5)

**Anthropic's own system cards for the 5.x models publish no MRCR, GraphWalks, RULER or other accuracy-vs-length curve.** I downloaded and text-searched all three (curl + pdftotext, 2026-09-18):
- Claude Opus 5 System Card (https://www-cdn.anthropic.com/c5fbac3f0b1280a933ebd26d3cb8bb9f5bdeaf48/Claude%20Opus%205%20System%20Card.pdf), §8.9 "Long context" contains only ProgramBench: "Claude Opus 5 scored 83% after the first episode, increasing to 93% by the fifth episode. For reference, Claude Opus 4.8 scored 80% … rising to 90% …, while Mythos 5 scored 84% and 93%." Episodes "cover a range of context lengths up to the full 1M token window" — a single aggregate, not a curve.
- Claude Sonnet 5 System Card (https://www-cdn.anthropic.com/480e0bb54327b9622282e9c39a83a4f490ed377e/Claude%20Sonnet%205%20System%20Card.pdf), §8.8 ProgramBench: "Claude Sonnet 5 scores 76–86%, compared to 52–74% for Claude Sonnet 4.6."
- Claude Fable 5.1 & Mythos 5.1 System Card (https://www-cdn.anthropic.com/0339e6a7c5c7b87f5c07798616dc32c215d14235/Claude%20Fable%205.1%20&%20Claude%20Mythos%205.1%20System%20Card.pdf), §8.11 ProgramBench: "Claude Fable 5.1 scored 87.6%, while Claude Opus 5 scored 85.4% and Fable 5 scored 86.3%." Its alignment section also notes "context compaction can further change how models act. We simulate compaction, but we have more limited coverage of long trajectories."
- Relevant to the threshold question: **Anthropic's own agentic evals of these 1M-window models compact at 200k.** Opus 5 card, BrowseComp: "To extend beyond the 1M-token context window, we used context compaction, triggered at 200k tokens." Sonnet 5 card, Table 8.1.A: "BrowseComp uses a 10M-token limit with context compaction (triggered at 200k)." (Other evals in those cards used a 1M budget without compaction.)
- Anthropic API server-side compaction (`compact_20260112`, supports opus-5, sonnet-5, fable-5-1 and others): **default `trigger` 150,000 input tokens, minimum 50,000**; docs: "As a conversation grows, response quality degrades, so compaction replaces older content with a concise summary." (https://platform.claude.com/docs/en/build-with-claude/compaction). Note this differs from Claude Code's 967k default for the same models.
- Model pages give qualitative claims only: Opus 5 "consistent instruction following, tool calling, and reasoning throughout the window" (https://platform.claude.com/docs/en/models/opus-5/whats-new-opus-5); Fable 5.1 "reasoning over and connecting details across the full 1M token context window" (https://platform.claude.com/docs/en/models/fable-5-1/whats-new-fable-5-1).

**Measured curves that do exist:**
- **Opus 5 and Sonnet 5: third-party, Context Arena MRCR v2 8-needle at max effort (1.0a).** Opus 5 holds ≥.91 through 64K–128K, then .656 at 128K–256K and .425 at 256K–512K (90% and 70% points both in the 128K–256K bin). Sonnet 5 is already at .680 in the 32K–64K bin (90% point 32K–64K; 70% point 64K–128K).
- **Fable 5.1: no measured curve found anywhere** (not on Context Arena; not in its system card). The GPT-5.6 launch post has GraphWalks BFS for "Claude Mythos 5" (a separate model from Fable 5, per that page's own column headers): 91.1% at 256k, 79.4% at 1M (87.2% ratio; 90% point in (256k, 1M]) (https://openai.com/index/gpt-5-6/ ✔). Treat as a weak cross-model proxy.
- GraphWalks BFS in the same OpenAI table: Claude Opus 4.8 85.9% (256k) → 68.1% (1M) (79.3%) ✔.
- PROXY, first-party full table — Claude Sonnet 4.6 System Card §2.16 (https://www-cdn.anthropic.com/78073f739564e986ff3e28522761a7a0b4484f84.pdf, ✔ via pdftotext): "256K" bin = (128k, 256k], "1M" bin = (524k, 1024k].

  | model (config) | MRCR v2 8-needle 256K | 1M | GraphWalks BFS 256K / 1M | Parents 256K / 1M |
  |---|---|---|---|---|
  | Sonnet 4.6 (max) | 90.3 | 65.8 | 74.5 / 73.8 | 97.9 / 86.4 |
  | Opus 4.6 (max) | 93.0 | 76.0 | 61.1 / 38.7 | 95.4 / 72.0 |
  | Sonnet 4.5 (64k thinking) | 10.8 | 18.5 | 44.9 / 25.6 | 81.0 / 50.2 |

  MRCR: 90% point between 256K and 1M for both 4.6 models, 70% not reached. GraphWalks BFS: Sonnet 4.6 barely moves (99%), Opus 4.6 falls to 63% — degradation is model-specific. The card's 1M MRCR results are "not reproducible via the public API, as some problems exceed its 1M token limit."
- Source quality: Opus 5, Sonnet 5 **measured, third-party (max effort only)**; Fable 5.1 **unknown** (cross-model proxy only); vendor-side shape **proxy** (4.6-generation card).

### 1.3 Qwen3.8 27B (`ollama/qwen3.8`, pi)

- What it is (not Qwen3 8B, see 2.5): Qwen3.8-27B, hybrid Gated-DeltaNet / Gated-Attention, native 262,144 context, extensible to ~1M only with opt-in YaRN; HF README warns "All the notable open-source frameworks implement static YaRN … potentially impacting performance on shorter texts" (https://huggingface.co/Qwen/Qwen3.8-27B/raw/main/README.md, https://huggingface.co/Qwen/Qwen3.8-27B/raw/main/config.json — sub-agent). The Ollama build does not enable YaRN (`rope.freq_base 10000000`, context 262,144 from `/api/show`).
- **First-party long-context numbers: not found** (Qwen's repo shows benchmarks only as images; sub-agent).
- **Third-party, the actual model: Context Arena MRCR v2 8-needle (1.0a) ✔**, full-precision via AkashML: flat at ≥.93 through 32K–64K for low/medium effort, then .79–.82 in the 64K–128K bin (90% point 64K–128K; xhigh crosses one bin earlier). Nothing measured beyond 128K, although pi compacts at 245,760.
- Without thinking the same model starts at .603 at 4K–8K; the fleet's pi launch passes `--thinking` from the teammate's `effort`.
- 4-bit quantization: a general-benchmark study found Q4_K_M "matched BF16 performance within confidence intervals" on GPQA Diamond and Terminal-Bench (https://quesma.com/blog/qwen38-27b-quantizations-benchmarked/, sub-agent); **no long-context measurement of the quantized build found.**
- Siblings on the same Context Arena JSON ✔: Qwen3.6-27B (enabled) .976 → .566 by 64K–128K; Qwen3.5-27B (enabled) 1.000 → .422 by 64K–128K; Qwen3.8-Max (medium) .993 → .873 (128K) → .706 (256K) → .257 (512K).
- Source quality: **measured, third-party, ≤128K only**; >128K **unknown**.

### 1.4 NVIDIA Nemotron 3 Ultra (`opencode/nemotron-3-ultra-free`)

What it is: NVIDIA Nemotron 3 Ultra, 550B total / 55B active hybrid Mamba-Attention MoE, context extended to 1M tokens (continued pretraining on 1,048,576-token sequences). HF card: "Up to 1M tokens". https://arxiv.org/html/2606.15007 ; https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16

| benchmark | checkpoint | length | score | URL |
|---|---|---|---|---|
| RULER | **Base** | 64K | 95.30 ✔ | https://arxiv.org/html/2606.15007 (Table 2) |
| RULER | Base | 128K | 92.49 ✔ | same |
| RULER | Base | 256K | 86.22 ✔ | same |
| RULER | Base | 512K | 84.54 ✔ | same |
| RULER | Base | 1M | 76.83 ✔ | same |
| RULER | **Post-trained** (the served type) | 1M | 94.7 ✔ | https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16 |
| LongBench v2 | Post-trained | "≤ 1M" (mixed lengths) | 65.9 ✔ | same card |
| AA-LCR | Post-trained | ~100K fixed | 65.4 ✔ | same card |

- Base curve: 90% point = **512K** (bracket (256K, 512K]); 70% point **not reached by 1M** (76.83 = 80.6% of 95.30).
- Post-trained checkpoint: only one point (94.7 @1M), so no curve. It is 18 points above the Base checkpoint at 1M, so the Base curve likely overstates the served model's decline. RULER is a synthetic retrieval/aggregation test and is known to be easier than MRCR-style tasks (see 1.7).
- Not listed on contextarena.ai or Fiction.LiveBench (sub-agent check).
- Source quality: **measured, first-party** (base curve) / single point (served model).

### 1.5 NVIDIA Nemotron 3.5 Lightning (`opencode/nemotron-3.5-lightning-free`)

What it is: 30B total / 3B active hybrid Mamba-2 + MoE + Attention, distilled from Nemotron 3 Ultra; card: "supports up to 1M context length", with the note "for single H100 deployment, we use 256K" (https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16). OpenCode declares 262,144.

| benchmark | model | length | score | URL |
|---|---|---|---|---|
| RULER | Nemotron-3.5 Lightning (card does not say base vs. post-trained) | 256K | 76.88 ✔ | https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-Base-BF16 |
| RULER | same | 1M | 69.62 ✔ | same |
| AA-LCR | post-trained BF16 | ~100K fixed | 52.00 ✔ | https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16 |
| (same card, comparison columns) RULER 256K / 1M | Qwen3.5-35B-A3B 82.36 / 56.43; Gemma-4-26B-A4B 85.73 / 72.93; Nemotron-3 Nano 30B-A3B 71.71 / 51.23; Nemotron-3 Super 120B-A12B 83.03 / 66.98 | | ✔ | Base card above |

- Only two points, both at or above OpenCode's 262,144 window. 1M is 90.6% of 256K, so no 90% crossing is visible between 256K and 1M. **Nothing is published below 256K**, i.e. inside the window the fleet actually uses.
- PROXY (Nemotron 3 Nano 30B-A3B, same size class, *different model*), https://arxiv.org/html/2512.20848: Base RULER 87.50 @64K → 82.92 @128K → 75.44 @256K (90% point **256K**, bracket (128K, 256K]); post-trained "RULER-100" 92.92 @256K → 91.25 @512K → 86.34 @1M (no 90% crossing).
- Third-party PROXY (weak): the current Context Arena JSON (1.0a) has only `nemotron-3-super-120b-a12b` from this family: .541 at 4K–8K, .381 at 64K–128K ✔ — low from the first bin, so the ratio thresholds say little; in absolute terms it is far below the other fleet models on MRCR. (A sub-agent also reported a Nano row on the older `old.contextarena.ai`: 0.0% at 128K on one probe domain; not re-checked.) MRCR is much harder than RULER.
- Source quality: **measured, first-party, sparse (2 points ≥256K)**; below-256K shape **unknown** (proxy only).

### 1.6 `opencode/big-pickle`

- **Identity undisclosed.** OpenCode Zen docs: "a stealth model that's free on OpenCode for a limited time… The team is using this time to collect feedback and improve the model" (https://opencode.ai/docs/zen/). models.dev entry gives only limits (context 200,000, input 160,000, output 32,000, release 2025-10-17) (https://github.com/sst/models.dev/blob/dev/providers/opencode/models/big-pickle.toml). Issue "Is zen/big-pickle glm 4.6?" was closed with no maintainer answer (https://github.com/anomalyco/opencode/issues/4276).
- Unconfirmed community claims that it is Zhipu GLM-4.6 exist (X posts linked from the issue thread and elsewhere); **not confirmed by OpenCode**. If that claim were true, the nearest published curve would be GLM-4.6 on LongBench Pro (arXiv 2601.02872): 53.74 @8k → 50.76 @16k → 51.93 @32k → 45.60 @64k → 37.73 @128k (90% point 64k, 70% point just past 128k). The sub-agent could not render the table itself and got these digits via search-tool synthesis only; treat as **unverified** and do not attribute them to big-pickle.
- No benchmark is published under the big-pickle name on contextarena.ai, Fiction.LiveBench, or OpenCode docs.
- Source quality: **unknown**.

### 1.7 Model-agnostic evidence that accuracy degrades before the window is full

- **Chroma, "Context Rot"** (https://www.trychroma.com/research/context-rot, July 2025; 18 models, Claude 4 / GPT-4.1 / o3 / Gemini 2.5 / Qwen3 generation — older than every fleet model): "models do not use their context uniformly; instead, their performance grows increasingly unreliable as input length grows"; "Across all models, we see significantly higher performance on focused prompts compared to full prompts" (LongMemEval, ~113k-token full prompts vs. focused ones). Qualitative/graph-based; no per-length table. (sub-agent)
- **NoLiMa** (arXiv 2502.05167; table at https://github.com/adobe-research/NoLiMa): "effective length" = longest length scoring ≥85% of the model's base score. GPT-4.1 (claims 1M): effective **16K** ✔; GPT-4o: 8K; most others ≤2K. "At 32K, for instance, 10 models drop below 50% of their strong short-length baselines." Newest rows: GPT-4.1, Llama 4, Gemini 2.5 Flash — no 2026 models.
- **RULER** (arXiv 2404.06654; https://github.com/NVIDIA/RULER): effective length = longest length above Llama-2-7B's 4K score (85.6%). README: "Almost all models fall below the threshold before reaching the claimed context lengths." ✔. Newest rows are the Qwen3 family, e.g. Qwen3-8B 96.3 (4K) → 82.1 (64K) → 77.4 (128K), effective 64K ✔. RULER is easy relative to MRCR: the same generation's models look far better on RULER than on MRCR (compare Nemotron in 1.4–1.5 with Context Arena in 1.0a).
- **Anthropic**: context-engineering post — "as the number of tokens in the context window increases, the model's ability to accurately recall information from that context decreases" (https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents ✔); API compaction docs — "As a conversation grows, response quality degrades" (1.2). Claude Code docs frame the 967k default as a cost/continuity choice ("The auto setting picks a window tuned for your model and is strongly recommended for the best cost and performance", binary string ✔).
- **Fiction.LiveBench**: table is an image; no rows for any fleet model could be extracted; last changelog entry 2026-04-04, before most fleet models shipped. **Not usable.**

---

## 2. Harness defaults: auto-compaction trigger and the knob that moves it

### 2.1 Summary

"Trigger" means the token count at which the harness starts an automatic compaction, for the model the fleet actually runs. "% of window" is that count divided by the context window the harness believes the model has.

| harness · model | window the harness uses | default trigger (tokens) | % of window | knob that moves it (exact name, where it lives) | can the knob compact *earlier*? |
|---|---|---|---|---|---|
| Claude Code · `opus` (claude-opus-5) | 1,000,000 | **967,000** | 96.7% | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (env, teammate `env:`), `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (env), `autoCompactWindow` (settings.json, set by `/autocompact`), `--autocompact` (CLI flag) | yes, all four |
| Claude Code · `sonnet` (claude-sonnet-5) | 1,000,000 | **967,000** | 96.7% | same | yes |
| Claude Code · `fable` (claude-fable-5-1, orchestrator) | 1,000,000 | **967,000** | 96.7% | same | yes |
| Codex · `gpt-5.6-sol` | 272,000 (catalog `context_window`; `max_context_window` 872,000) | **244,800** | 90% | `model_auto_compact_token_limit` (`~/.codex/config.toml`, or `-c` flag) | yes (only lower; it is `min`-ed with 90%) |
| Codex · `gpt-5.6-terra` | 272,000 (max 872,000) | **244,800** | 90% | same | yes |
| OpenCode · `nemotron-3.5-lightning-free` | 262,144 (no `input` limit) | **230,144** | 87.8% | `compaction.auto`, `compaction.reserved` **(ignored for this model as shipped)**, `provider.opencode.models.<id>.limit.context` or `.limit.input` (opencode.json / `OPENCODE_CONFIG_CONTENT`) | only via a model-limit override (`limit.context`, or add `limit.input`, after which `reserved` applies) |
| OpenCode · `nemotron-3-ultra-free` | 1,000,000 (no `input` limit) | **968,000** | 96.8% | same as above | only via a model-limit override (as above) |
| OpenCode · `big-pickle` | 200,000 context, 160,000 input | **140,000** | 70% of context (87.5% of input) | `compaction.reserved` (applies here), `limit.input` override | yes (`reserved`) |
| pi · `ollama/qwen3.8` | 262,144 (`~/.pi/agent/models.json`) | **245,760** | 93.75% | `compaction.reserveTokens` in `~/.pi/agent/settings.json` or `<project>/.pi/settings.json` | yes (raise `reserveTokens`) |
| Prime · `anthropic/claude-opus-5` | 1,000,000 (bundled registry) | **983,616** | 98.4% | `compaction.reserveTokens` in `~/.prime/agent/settings.json` or `<project>/.prime/agent/settings.json` | yes |

All harnesses **do** auto-compact by default. None is currently overridden on this machine (no `env` block in `~/.claude/settings.json`; `~/.codex/config.toml` has no compaction key; no `~/.pi/agent/settings.json`; `~/.prime/agent/settings.json` holds only `telemetry`).

### 2.2 Claude Code 2.1.274

**Measured on this account** (`claude -p "/autocompact" --model <m>` run in `/tmp`, 2026-09-18):

```
opus   -> Auto-compact window: 1m tokens (default for this model)
sonnet -> Auto-compact window: 1m tokens (default for this model)
fable  -> Auto-compact window: 1m tokens (default for this model)
CLAUDE_CODE_AUTO_COMPACT_WINDOW=200000 (opus) -> Auto-compact window: 200k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW)
CLAUDE_CODE_DISABLE_1M_CONTEXT=1 (opus)        -> Auto-compact window: 200k tokens (default for this model)
```

No "Auto-compact is currently disabled" line was printed, so auto-compact is on. **Replicated with fleet launch conditions** (project root; `--effort xhigh --permission-mode auto --settings '{"enabledPlugins":{…all false}}'`, i.e. the overlay horch builds; no teammate sets `setting_sources`, so user settings load): opus, sonnet and fable all again report `1m tokens (default for this model)` and not disabled. Caveat: probed in `-p` mode; interactive panes use a different `CLAUDE_CODE_ENTRYPOINT`, which only matters for Sonnet 5's per-surface default (500k on `remote_cowork` / `local-agent`, 1M otherwise). (Note: `~/.claude.json` still carries a legacy `autoCompactEnabled: false`, but `~/.claude/settings.json` has `autoCompactEnabled: true`; the binary reads the key through the settings resolver with `userSettings` taking precedence over `legacyGlobalConfig`, and the live `/autocompact` output confirms it is enabled.)

**How the trigger is computed** (read from the binary; minified names in brackets so a reader can re-find them with `strings`):

1. Resolve the auto-compact *window* [`GE`], first match wins:
   `CLAUDE_CODE_AUTO_COMPACT_WINDOW` env (clamped to 100,000–1,000,000, then to the model window) → `autoCompactWindow` setting (written by `/autocompact`; `--autocompact` flag overrides it for one launch) → server "clientdata" value → GrowthBook experiment (only for `claude-opus-4-8`) → model default. Model default: 200,000 for `claude-sonnet-4-6 / opus-4-6 / opus-4-8 / opus-5` **only when** the model is running with a <1M window; `claude-sonnet-5` has a per-surface table (1M; 500k on `remote_cowork`/`local-agent` surfaces); any native-1M model on the first-party API gets its full 1M window.
2. Effective window `y = window − min(max_output_tokens, 20,000)`. For all three fleet Claude models the default max output is 64,000, so `y = window − 20,000` [`W6`, `Lkn=20000`].
3. Trigger `= y − 13,000`. With `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=p` (0 < p ≤ 100): trigger `= min(floor(y · p/100), y − 13,000)` — i.e. the percentage is taken of `y`, and it can only **lower** the trigger [`qPe`; the parsed value is held in a field literally named `testPctOverride`].
4. Warning shown at trigger − 20,000; hard block at (model window − 20,000) − 3,000 [`Rtt`].
5. A background "precompute" path arms at `min(y − round(0.2·y), trigger)` ≈ 80% of `y` (fraction `0.2` is a server-tunable default, `tengu_amber_rokovoko`), gated by a feature check; it prepares the summary early and later swaps it in (`precomputed_compact_swap`). Whether that path is live for this account was not verified.
6. Thrash guard: if context refills to the limit within 3 turns of a compact, 3 times in a row, auto-compact trips with "Autocompact is thrashing … A file being read or a tool output is likely too large for the context window."

Worked numbers (1M models): default `980,000 − 13,000 = 967,000`. `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=50` → 490,000. `CLAUDE_CODE_AUTO_COMPACT_WINDOW=200000` → 167,000. `=300000` → 267,000. `=100000` (minimum) → 67,000. `CLAUDE_CODE_DISABLE_1M_CONTEXT=1` → 167,000.

**Does the 1M option change the base the percentage is taken from?** Yes. The percentage applies to the *resolved window minus the output reserve*, and for native-1M models on the first-party API that window is 1,000,000. Docs (https://code.claude.com/docs/en/model-config, "Default auto-compact thresholds"): "Models running with a native 1M window, such as Sonnet 5, the Fable models, and Opus 4.7 and later on the Anthropic API, compact before the window fills, at about 967K tokens by default." The window falls to 200K when `CLAUDE_CODE_DISABLE_1M_CONTEXT=1`, when `ANTHROPIC_BASE_URL` points anywhere other than `api.anthropic.com` (binary check [`Xo`/`mw`]), or when the account's 1M credits are blocked.

**Knob verification:**

| knob | verified by | default | where it lives | effect |
|---|---|---|---|---|
| `CLAUDE_CODE_AUTO_COMPACT_WINDOW` | (1) binary: `CLAUDE_CODE_AUTO_COMPACT_WINDOW is set and takes precedence. Unset it to change this setting.`; (2) https://code.claude.com/docs/en/model-config ("While it's set, it takes precedence over the command, the flag, and the setting"); measured with `/autocompact` above | unset → model default (1M here) | teammate `env:` map (per worker) or shell env | Sets the window in tokens (100k–1M, plain integer only). Trigger = value − 33,000. The binary prints "Overriding auto may result in high token usage, especially when resuming long sessions." when env/settings override it. |
| `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` | (1) binary (`process.env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` → `parseFloat`); (2) https://code.claude.com/docs/en/env-vars: "Set the percentage (1-100) of the auto-compact window at which auto-compaction triggers. Use lower values like `50` to compact earlier; the variable can't raise the threshold … Applies to both main conversations and subagents" | unset → 96.7% | teammate `env:` map | Percent of `y`; lower-only. |
| `autoCompactWindow` / `/autocompact` / `--autocompact` | (2) https://code.claude.com/docs/en/model-config ("Set the auto-compact window") | `auto` | `settings.json` (user scope when set via `/autocompact`) or CLI flag | Same unit as the env var; env var beats all three. |
| `autoCompactEnabled`, `DISABLE_AUTO_COMPACT`, `DISABLE_COMPACT` | (1) binary; the `autocompact_state` schema string says "autoCompactEnabled setting + DISABLE_AUTO_COMPACT / DISABLE_COMPACT env; DISABLE_COMPACT also disables manual /compact" | enabled | settings.json / env | On/off only. |
| `CLAUDE_CODE_DISABLE_1M_CONTEXT` | (1) binary [`YH`]; (2) model-config docs; measured above | unset | env | Holds native-1M models to a 200K window → trigger 167,000. |

### 2.3 Codex CLI 0.154.0

- **Knob:** `model_auto_compact_token_limit` (integer tokens) in `~/.codex/config.toml` (horch gives each pane a private `CODEX_HOME`, so it would go in that pane's `config.toml` or a `-c model_auto_compact_token_limit=N` launch flag). Companion: `model_auto_compact_token_limit_scope = "total" | "body_after_prefix"` (default `total`), `model_context_window`, `compact_prompt`, `experimental_compact_prompt_file`.
- **Verified by:** (1) binary strings `model_auto_compact_token_limit`, `model_auto_compact_token_limit_scope`, `AutoCompactTokenLimitScope total body_after_prefix`; (2) https://learn.chatgpt.com/docs/config-file/config-reference (redirect target of `developers.openai.com/codex/config-reference`, which the repo's `docs/config.md` points to): "Token threshold that triggers automatic history compaction (unset uses model defaults)."
- **Default when unset** (source, `codex-rs/protocol/src/openai_models.rs` at `rust-v0.154.0`):
  ```rust
  pub fn auto_compact_token_limit(&self) -> Option<i64> {
      let context_limit = self.resolved_context_window().map(|context_window| (context_window * 9) / 10);
      let config_limit = self.auto_compact_token_limit;
      if let Some(context_limit) = context_limit {
          return Some(config_limit.map_or(context_limit, |limit| std::cmp::min(limit, context_limit)));
      }
      config_limit
  }
  ```
  So the default is **90% of the resolved context window**, and a configured value can only lower it. The embedded catalog in the binary lists `gpt-5.6-sol` and `gpt-5.6-terra` with `"context_window": 272000`, `"max_context_window": 872000`, `"auto_compact_token_limit": null` → default trigger **244,800**. Separately, the hard cap on active context is `effective_context_window_percent` (default 95) → 258,400.
- Raising `model_context_window` (capped at `max_context_window` 872,000, `models-manager/src/model_info.rs`) raises the 90% default with it (e.g. 872,000 → 784,800).
- With `scope = "body_after_prefix"` only growth after the carried compaction prefix is counted, and there the configured limit is used as-is (not `min`-ed with 90%) (`core/src/session/context_window.rs`).
- The binary also embeds a `token_budget` feature with `"auto_compact_fallback_buffer_tokens": 16384` and a notes-based fallback prompt ("The current context window is exhausted … Make exactly one write or append call to `notes` now…"); this is a fallback path, not the default trigger.

### 2.4 OpenCode 1.18.2

- **Trigger** (bundle, `SessionCompaction.isOverflow` → functions `Dl`/`Is`):
  ```js
  function Is(e){ let o=e.model.limit.context; if(o===0) return 0;
    let l = e.cfg.compaction?.reserved ?? Math.min(20000, maxOutputTokens(e.model, e.outputTokenMax));
    return e.model.limit.input ? Math.max(0, e.model.limit.input - l)
                               : Math.max(0, o - maxOutputTokens(e.model, e.outputTokenMax)) }
  function Dl(e){ if(e.cfg.compaction?.auto===false) return false; if(e.model.limit.context===0) return false;
    return (e.tokens.total || e.tokens.input+e.tokens.output+e.tokens.cache.read+e.tokens.cache.write) >= Is(e) }
  ```
  `maxOutputTokens = min(model.limit.output, 32000)` (`OUTPUT_TOKEN_MAX = 32000`, overridable by `OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX`). The check uses the provider-reported token usage of the last assistant message (cache reads count).
- **Model limits in use** (`~/.cache/opencode/models.json`, provider `opencode` = "OpenCode Zen"): `nemotron-3.5-lightning-free` context 262,144 / output 262,144 (no input limit) → 262,144 − 32,000 = **230,144**; `nemotron-3-ultra-free` context 1,000,000 / output 128,000 (no input limit) → **968,000**; `big-pickle` context 200,000 / **input 160,000** / output 32,000 → 160,000 − 20,000 = **140,000**.
- **Asymmetry worth knowing:** `compaction.reserved` is only consulted when the model declares `limit.input`. For both Nemotron models it is **ignored** as shipped; what moves their trigger is overriding the model's limits: either `limit.context` (trigger = context − min(output, 32,000)) or adding `limit.input` (trigger = input − `reserved`, so `compaction.reserved` then works), e.g. `{"provider":{"opencode":{"models":{"nemotron-3-ultra-free":{"limit":{"context":N,"output":128000}}}}}}` (schema `limit:{context, input?, output}` verified in the bundle). For big-pickle, `compaction.reserved` works directly.
- **Knobs** (bundle config schema, with its own descriptions): `compaction.auto` "Enable automatic compaction when context is full (default: true)"; `compaction.prune` "Enable pruning of old tool outputs (default: false)"; `compaction.tail_turns` "Number of recent user turns … to keep verbatim during compaction (default: 2)"; `compaction.preserve_recent_tokens`; `compaction.reserved` "Token buffer for compaction. Leaves enough window to avoid overflow during compaction." Env: `OPENCODE_DISABLE_AUTOCOMPACT` (sets `compaction.auto=false`), `OPENCODE_DISABLE_PRUNE`. Lives in opencode.json or horch's `OPENCODE_CONFIG_CONTENT`. Docs: https://opencode.ai/docs/config/ (the other researchers cover the full env list).

### 2.5 pi 0.85.1 and Prime Agent 0.9.4

Both compact by default. Installed docs `…/pi-coding-agent/docs/compaction.md` and `…/prime-agent/docs/compaction.md`:

> Auto-compaction triggers when: `contextTokens > contextWindow - reserveTokens`. By default, `reserveTokens` is 16384 tokens …

Settings (installed `docs/settings.md` of each): `compaction.enabled` (default `true`), `compaction.reserveTokens` (default `16384`), `compaction.keepRecentTokens` (default `20000`, recent tokens kept verbatim). pi: `~/.pi/agent/settings.json` or `<project>/.pi/settings.json`; Prime: `~/.prime/agent/settings.json` or `<project>/.prime/agent/settings.json`. There is no percentage knob; to compact earlier, raise `reserveTokens` (trigger = window − reserveTokens) or lower the model's declared `contextWindow`.

- pi · `ollama/qwen3.8`: `~/.pi/agent/models.json` declares `contextWindow: 262144`, `maxTokens: 32000` → trigger **245,760**.
  - **The model is not Qwen3 8B.** `ollama show qwen3.8` / `/api/show`: `general.basename "Qwen3.8"`, `general.size_label "27B"` (27.3B params), architecture `qwen35`, `general.version "0814"`, `qwen35.context_length 262144`, vision + tools + thinking, Q4_K_M, parent `qwen3.8:27b-q4_K_M`. pi's own models.json names it "Qwen3.8 27B (local)". (A separate `qwen3:8b` tag is also installed — native context 40,960 per `ollama show`, though pi's models.json declares 128,000 for it — but that is not the fleet tag.)
  - **Ollama actually serves 262,144 here**, so pi's trigger is reachable: `OLLAMA_CONTEXT_LENGTH` is unset (`ollama serve --help`: "default: 4k/32k/256k based on VRAM"), and the server logs record `msg="vram-based default context" total_vram="107.5 GiB" default_num_ctx=262144` (`~/.ollama/logs/server-1.log` 2026-08-15, `server-2.log` 2026-07-31; the newest server log on disk). Not re-measured with a live load today.
- Prime · `anthropic/claude-opus-5`: bundled registry `contextWindow: 1e6`, `maxTokens: 128e3` → trigger **983,616**. Prime's `docs/long-running-agents.md`: "The Python kernel persists through compaction, so variables, imports, helper functions, and task state remain available."

---

## 3. Cost of compacting

### 3.1 What one compaction costs (Claude Code, documented)

Source: https://code.claude.com/docs/en/prompt-caching ("Compacting the conversation", "Which TTL each request gets") and https://code.claude.com/docs/en/costs.

- **It is one extra request over the whole context.** "To produce the summary, Claude Code sends a separate request with the same system prompt, tools, and history as your conversation, plus a summarization instruction appended as a final user message."
- **Cached vs. uncached input.** "While the cache is warm, that request reads your prefix from the cache, so a mid-session `/compact` costs a fraction of what the context size suggests and spends most of its time generating the summary." Cache reads bill at "roughly 10% of the standard input rate". "After a break longer than the cache lifetime, there is no cache left to read, so the summarization request reprocesses the full history as uncached input."
- **Output side.** The summary is generated output; since v2.1.198 the summarization request "inherits your session's extended thinking configuration" (https://code.claude.com/docs/en/context-window#what-survives-compaction), so with thinking on, the summary also spends thinking tokens.
- **Cache rebuild afterwards is small.** "By design, this invalidates the conversation layer … Claude Code reuses the system prompt layer … the turn after compaction rebuilds the conversation cache for only the much shorter summary, so that turn is not the slow part."
- **TTL bucket.** Compaction is in the "everything else" bucket: 5-minute TTL by default even on a subscription (main conversation gets 1 hour on a subscription within plan usage). The summarization request shares the main conversation's prefix, so it reads the main conversation's cache while that is warm.
- **Costs doc, "Why usage climbs in a long session":** "`/compact` reads the conversation it summarizes, so compacting a large context is itself a large request. When you want a fresh start instead of continuity, `/clear` costs nothing."
- **Timing advice in the docs:** "run `/compact` at a natural break in your work, such as between tasks, instead of waiting for auto-compaction to trigger mid-task."
- Scale, for a sense of size: compacting at the 967k default reads ~967k tokens (mostly cache reads if warm); compacting at 200k reads ~200k. The per-turn cost of *not* compacting is the same effect in reverse: every turn re-sends the whole context at the cache-read rate (costs doc: "Claude Code sends your full conversation with every request").

### 3.2 What a compaction loses (Claude Code, documented)

https://code.claude.com/docs/en/context-window#what-survives-compaction (table, abridged):

| content | after compaction |
|---|---|
| System prompt, output style | still apply |
| Project-root CLAUDE.md, unscoped rules, auto memory, the plan-mode plan | re-injected from disk |
| Path-scoped rules, nested CLAUDE.md | reloaded only when a matching file is read again; otherwise summarized away |
| Files Claude read or edited | "re-reads up to five, most recently modified first"; a file over 5,000 tokens "comes back as a path reference without its content" |
| Invoked skill bodies | re-injected, "capped at 5,000 tokens per skill and 25,000 tokens total; oldest dropped first" |
| Background commands / background subagents | keep running; Claude is reminded which ones |
| Context hooks added earlier | summarized with the rest |
| `SessionStart` hooks matching `compact` | re-run, output added |

Everything else — tool results, in-flight reasoning, exact error text, which files were *not* worth reading, and any conversational state not written to disk — survives only as far as the summary captures it. Anthropic's engineering post (https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents): compaction keeps "architectural decisions, unresolved bugs, and implementation details while discarding redundant tool outputs or messages", and "overly aggressive compaction can result in the loss of subtle but critical context whose importance only becomes apparent later." The same post calls tool-result clearing the "safest lightest touch" form of compaction, and describes sub-agents returning "condensed, distilled summary of its work (often 1,000-2,000 tokens)".

Binary detail (2.1.274): a thrash guard trips if context refills to the limit within 3 turns of a compaction, 3 times running ("Autocompact is thrashing … A file being read or a tool output is likely too large for the context window"). A precompute path can prepare the summary at ≈80% of the effective window and swap it in at the trigger (see 2.2), which would move the summarization call earlier than the swap; not verified live.

### 3.3 Other harnesses

- **Codex 0.154.0:** for providers that support it (OpenAI), auto-compaction uses the provider's remote compaction endpoint (`compact_remote.rs`, `compact_remote_v2.rs`; `RemoteCompactionSupport::V2`), otherwise a local summarization turn (`compact.rs`); a `token_budget` feature swaps in a notes-based reset instead (`core/src/session/turn.rs::run_auto_compact`, tag `rust-v0.154.0`). Either way the full history is sent once more to produce the compacted form. Knobs to shape what survives: `compact_prompt`, `experimental_compact_prompt_file`.
- **OpenCode 1.18.2:** compaction keeps the last `compaction.tail_turns` (default 2) user turns verbatim, optionally capped by `compaction.preserve_recent_tokens`, and summarizes the head; `compaction.prune` (default false) separately drops old tool outputs (constants in the bundle: `PRUNE_MINIMUM = 20000`, `PRUNE_PROTECT = 40000`).
- **pi / Prime:** installed `docs/compaction.md`: the summarizer sees everything before the cut point, keeps the last `keepRecentTokens` (default 20k) verbatim, tracks read/modified files cumulatively, and "Compaction and branch-summary requests use fresh routing session IDs and, where supported by the provider, disable prompt-cache writes because these one-off prompts are unlikely to be reused" (pi 0.85.1 wording). So the pi summarization call is not a cache write; whether it can *read* the conversation's cache depends on the provider (irrelevant for local Ollama, where there is no per-token bill). Prime keeps its Python kernel state across compaction (`docs/long-running-agents.md`).

### 3.4 Orchestrator vs. worker (framing for the orchestrator's decision)

- **Long-lived orchestrator (Fable 5.1, 1M window, default trigger 967k).** Its context carries the live "who owns what" map: which worker holds which task, pending questions, integration state. Compaction keeps only what the summary captures plus the re-injected items in 3.2; the ownership map is conversational state, not a file on disk, unless it is written to the horch ledger or a file. It compacts rarely because of the 1M window, so each compaction is a large request (~967k tokens read) and a large information loss at once. The evidence in section 1 is about retrieval accuracy at high fill, which is the cost of letting it run that long.
- **Short-lived worker.** horch's own orchestrator prompt already states the preference (`teammates/_base/fleet-orchestrator.md`): "STRONGLY PREFER FRESH sessions: new context windows are sharper and more efficient than old ones … carry the needed knowledge forward instead of resuming". A fresh worker starts with a system prompt plus a briefing (compare Anthropic's 1,000–2,000-token sub-agent summaries) and pays no summarization call. A worker compacted in place pays the full-context summarization call and continues with a lossy summary plus ≤5 re-read files. How often fleet workers actually reach their trigger was not measured for this report (the horch ledger / session transcripts would show it).
- The horch `horch done` summary and the session ledger already function as the "structured note-taking" pattern Anthropic describes; they are what survives a worker's end, the way a compaction summary survives in place.

---

## 4. Unverified items and gaps (kept out of the tables above)

- **big-pickle = GLM-4.6** — community claim only (X posts; closed issue https://github.com/anomalyco/opencode/issues/4276 with no maintainer answer). The GLM-4.6 LongBench Pro digits (arXiv 2601.02872) came from search-tool synthesis; the sub-agent could not render the table. Not used.
- **Claude Code precompute path** (summary prepared at ≈80% of the effective window, `precomputed_compact_swap`) — present in the 2.1.274 binary, gated by a feature check; not observed live.
- **Ollama effective context** — 262,144 taken from the VRAM-based default in server logs dated 2026-07-31 and 2026-08-15; no live load was run today.
- **Sub-agent-only items, URL given but not re-checked by me:** GPT-5.2 Thinking MRCR curve; Qwen3.8-27B HF README/config quotes; Quesma quantization result; Chroma quotes; Nemotron 3 Nano RULER table (arXiv 2512.20848); the older `old.contextarena.ai` Nano row.
- **Gaps with no public data found:** any accuracy-vs-length curve for Claude Fable 5.1 or Mythos 5.1; any data for Nemotron 3.5 Lightning below 256K; anything for big-pickle; Qwen3.8-27B beyond 128K and for the Q4_K_M build; Claude 5.x and GPT-5.6 at the efforts the fleet actually uses (Context Arena has `max` only); Fiction.LiveBench rows for any fleet model (the table is an image, last updated 2026-04-04).
- **Not resolved:** why vendor MRCR numbers run 20–30 points above Context Arena for the same bins (1.0a).
