---
name: antigravity
brief_description: Gemini 3.8 Flash on agy, general work. TRAINS ON INPUT unless opt-out confirmed; treat like opencode-*.
generic: true
base: fleet-worker
agent: antigravity
# `agy models` lists each model once per thinking level
# (gemini-3.8-flash-low|medium|high). The bare id plus --effort selects the
# same model; agy 1.2.17 refuses --effort for an id it does not know.
# 3.8 Flash scores at or above 3.1 Pro on coding and agent benchmarks for
# about a third of the price (checked 2026-10-05).
model: gemini-3.8-flash
# Required: agy 1.3.0 refuses a bare model id without --effort, and
# `horch teammates --check` fails such a teammate. 3.8 Flash takes
# low|medium|high. medium is the lowest level that keeps general
# implementation work (multi-file edits, tests): low is for short answers.
# medium is also Google's default for code and agents.
effort: medium
# auto = --sandbox --dangerously-skip-permissions: no approval prompt in a pane
# nobody watches, and terminal commands stay in agy's OS sandbox.
permission_mode: auto
# A personal Google account's interactions can train Google's models unless
# the operator turns Activity and Telemetry off. Until the operator confirms
# that, the orchestrator sends this tier public work only, as for opencode-*.
trains_on_input: true

# No phase and no skills: agy has no flag or variable that points it at a
# skills directory, so horch exposes no bundled skills to it. agy still loads
# the operator's own ~/.gemini/antigravity-cli skills, plugins and GEMINI.md;
# it has no switch to turn them off, so inherit_plugins is not set.
#
# agy asks "Do you trust the contents of this project?" on the first launch in
# each directory, and horch cannot answer it. Trust the worktree once first.
---
Your tier: ANTIGRAVITY - Google's agy CLI on the operator's Google login.
General implementation work on public code. Keep changes minimal, and say
plainly when a task needs a model or a data policy this tier does not have.
