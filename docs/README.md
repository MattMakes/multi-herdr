# The horch handbook

This handbook is for an engineer who is new to this repository. After it you
can find where each concern lives, run and read the gate, and make the 5
most common changes with a step-by-step recipe.

The top-level [README](../README.md) is the user guide: how to install horch
and run a fleet. This handbook is the contributor guide.

## Start here

Read these 4 pages in order:

1. [architecture.md](architecture.md): the 2 binaries, the crates, the
   `horch-core` modules, where state lives, and the rules the tests enforce.
2. [command-flow.md](command-flow.md): every command as a diagram, and which
   commands need herdr, a fleet or model money.
3. [testing-and-gates.md](testing-and-gates.md): the gate (never under
   `git rebase -x`), the gate slots, oracles, goldens, `HORCH_BLESS`, fakes and
   the e2e gotchas.
4. [fleet-workflow.md](fleet-workflow.md): how the orchestrator runs units
   of work with plans, worktrees, the merge protocol and `STATUS.md`.

## Recipes

Each recipe names every file to change and the check for each step.

| recipe | for |
|---|---|
| [add-harness.md](recipes/add-harness.md) | a new agent CLI that workers can run |
| [add-skill.md](recipes/add-skill.md) | a new bundled skill |
| [add-teammate.md](recipes/add-teammate.md) | a new persona in `teammates/` |
| [add-command.md](recipes/add-command.md) | a new `horch` or `multi-herdr-dataset` command |
| [add-dataset-event.md](recipes/add-dataset-event.md) | a new event in the dataset event log |

## Reference pages

- [skills-and-teams.md](skills-and-teams.md): vendored skills and
  re-vendoring, the roster fields (`available_skills`, `operator_skills`,
  `offer_when`, `requires`), `horch doctor`, the Unreal, Swift and design
  teams, `horch agent-list`, the Antigravity harness and
  `multi-herdr-dataset`.
- [phase-skills.md](phase-skills.md): the phase catalogs, how each harness
  sees skills, and context measurements.
- [runtime-skill-checks.md](runtime-skill-checks.md): native skill discovery
  checked against the installed harnesses, with no model request.

## Designs

- `ai_docs/designs/2026-10-02-architecture-refactor-design.md`: the module
  boundaries and the `arc_*` rules.
- `ai_docs/designs/2026-10-02-dataset-competition-design.md`: the dataset
  mode, events, round states, judging and promotion.
- `ai_docs/designs/2026-09-28-fleet-telemetry-design.md`: telemetry, usage
  limits and routing.

## One rule above all

Never use `ANTHROPIC_API_KEY`. Every `claude` run uses the operator's
claude.ai login. When you run `claude` yourself, use
`env -u ANTHROPIC_API_KEY claude ...`.
