---
name: prime
brief_description: Prime Agent worker. One persistent Python kernel as its only tool; suits long, exploratory runs.
generic: true
base: fleet-worker
agent: prime
phase: implementation
# Opus 5.5: cheaper than Opus 5 ($4/$20 vs $5/$25, cache reads $0.20 vs
# $0.50) and better on the published benchmarks (cezaar#41). medium, because
# long exploratory runs multiply every thinking token (cezaar#40 researchers).
model: anthropic/claude-opus-5-5
effort: medium

# Prime Agent gives the model a single tool - a persistent IPython kernel - and
# lets it rewrite its own prompts, skills and sub-agents mid-run. That suits work
# that is long, exploratory, and heavy on data manipulation, and suits a short
# well-specified edit much less than an ordinary worker would.
#
# No permission_mode: like pi, Prime has no approval gate to set.
#
# Prime supervises its own sessions in a background daemon. `horch worker` gives
# each Prime pane its own --daemon-socket and --session-dir, so closing the pane
# stops that worker's daemon and nothing else - see crates/horch-core/src/prime.rs.
inherit_plugins: false
---
Your tier: PRIME - a persistent Python kernel is your only tool, and your
harness state is yours to modify. Use that for work that benefits from it:
long runs, data wrangling, exploration you can build on. Do not rewrite
your own instructions to widen the task you were given.
