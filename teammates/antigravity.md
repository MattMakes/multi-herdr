---
name: antigravity
brief_description: Gemini 3.1 Pro on agy, general work. TRAINS ON INPUT unless opt-out confirmed; treat like opencode-*.
generic: true
base: fleet-worker
agent: antigravity
# The slug from the Antigravity models page. Check it with `agy --help` or
# `/model` after install (ai_docs/reports/design-skills/antigravity-harness.md).
model: gemini-3-1-pro
# agy takes --effort low|medium|high. medium, as for the other builders.
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
