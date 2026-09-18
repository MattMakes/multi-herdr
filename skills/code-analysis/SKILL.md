---
name: code-analysis
description: Use when assessing changed code for complexity, maintainability, or configured static-analysis violations.
---

# Code Analysis

Analyze the assigned scope using the target repository's available analyzers. This portable adaptation does not bundle the upstream JavaScript metrics engine or install tools at runtime.

1. Determine the exact files or diff from the brief. Inspect language manifests, analyzer configuration, and CI scripts. Exclude generated/vendor files unless specifically in scope; report if no applicable source remains.
2. Run the existing project analysis command with its configured thresholds. Capture tool/version, command, scope, diagnostics, and exit code. Do not assume one language's metrics are available for another, or invent measurements when an analyzer is absent.
3. Group diagnostics by file and function. Read each flagged function and its callers to identify the actual construct driving the result: nested decisions, long-lived state, mutable globals, coupled calls, duplicated transformations, or unclear responsibility.
4. Interpret metrics according to the tool: cyclomatic complexity measures paths; live variables and variable span reflect state burden; ABC reflects assignments, calls, and conditions; nesting weights structural depth. A maintainability index may use lower-is-worse thresholds. Report only measurements the installed tool actually produces.
5. Distinguish new violations from the baseline where comparison is available. A low branch count can still hide excessive sequential state; identify this qualitatively rather than assigning invented scores. Conversely, a threshold violation alone does not prove a bug.
6. Report `file:line`, function, measured value/threshold when available, concrete cause, behavioral constraints, and a targeted refactoring recommendation. If no analyzer is available, perform an explicitly labeled qualitative review and report the measurement limitation.

Validation work reports findings by default. When refactoring is authorized, preserve contracts and meaningful tests, make focused changes, then rerun the same analysis and relevant tests. Compare before/after evidence without trading readable code for metric gaming. Save the report at the assigned path or `ai_docs/reviews/code-analysis.md`.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
