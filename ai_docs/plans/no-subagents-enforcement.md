# Fleet workers must not spawn subagents

GOAL: No fleet pane can start a subagent, a forked session, or an in-harness
delegated task. A worker that needs more hands sends `QUESTION:` to the
orchestrator, who splits the work and spawns. The rule is enforced by the
harness switches, not only by prose, and `horch teammates --check` fails a
teammate that lacks the switch.

## CONTEXT

The operator watched fleet workers spin up subagents (the Claude Code
`Agent` tool). That hides work from the ledger and the grid, and it takes a
decision that belongs to the orchestrator. The base briefings say "do not"
in prose; that was not enough.

Depends on: PR 5, branch `bake-in-orchestration`, commits `5d75fac` and
`26681a1`. It adds the prose rule to `teammates/_base/fleet-worker.md` and
edits the same teammate frontmatter files this task edits. PR 5 is open,
not merged. Build on its branch, not on `main`.

WARNING: the shared checkout at `/Users/mascott/projects/multi-herdr` is
used by other live sessions. Never run `git checkout` or `git switch` in
it. Use a worktree:

```
git -C /Users/mascott/projects/multi-herdr worktree add /Users/mascott/projects/multi-herdr-no-subagents -b no-subagents bake-in-orchestration
```

Do all edits, builds, and tests inside `/Users/mascott/projects/multi-herdr-no-subagents`.
Run `horch note`, `horch tell`, and `horch done` from the pane's own
directory, `/Users/mascott/projects/multi-herdr`, so the ledger and the
role resolve. Use `git -C <worktree>` and `(cd <worktree> && cargo ...)`
for the rest. Every file path in FILES below is relative to the worktree.

Open the PR with `--base bake-in-orchestration`. The orchestrator retargets
it to `main` after PR 5 merges. After the PR is open, run
`git -C /Users/mascott/projects/multi-herdr worktree remove /Users/mascott/projects/multi-herdr-no-subagents`.

PRIOR WORK (from the ledger, worker opus-1):
- `~/.local/bin/horch` is first on PATH and was built on 2026-09-19. It
  predates the `orchestrate` skill and fails `teammates --check` on this
  roster. Always call the binary you built: `target/release/horch` or
  `target/debug/horch` inside the worktree.
- `ANTHROPIC_API_KEY` is set in this shell and is invalid. A nested
  `claude -p` call returns 401. Prefix every `claude` command in Step 1 with
  `env -u ANTHROPIC_API_KEY`. Do not use that key.
- `teammates/_base/fleet-orchestrator.md` now carries a
  `skills_instruction` frontmatter field. `prompts::worker_prompt` bails
  when a teammate declares `skills:` and its base has none. Keep it.
- The golden tests use named sanctioned-difference blocks, not refreshed
  captures. If a golden assertion breaks, add a block; never rewrite a
  golden file.
- `opus.md` and `sonnet.md` carry 4 `disabled_skills` names; the other 9
  Claude teammates carry 2. Merge your `disallowed_tools` line next to
  them without changing those lists.

What exists (read before editing):
- `teammates/_template.md:102-116`: Claude-only fields `tools`,
  `allowed_tools`, `disallowed_tools`. `disallowed_tools` becomes
  `--disallowedTools` at `crates/horch-core/src/launch.rs:260-262`, gated
  by `agent.takes_tool_denylist()` at `launch.rs:172`.
- `teammates/_template.md:176-177`: `args` and `env` per teammate.
- `crates/horch-core/src/teammates.rs:691` `pub fn check(&self) -> Vec<String>`
  is the roster check that `horch teammates --check` runs. Rules push into
  `problems`.
- `ai_docs/plans_to_improve.md:214` names `CLAUDE_CODE_FORK_SUBAGENT=false`
  as a Claude env switch. Verify it against `claude --help` and the docs
  under `ai_docs/reports/env-research/claude-code.md` before you use it.
- `crates/horch-core/src/codex.rs`, `opencode.rs`, `prime.rs`, and the
  `pi` launch path in `launch.rs` show what each harness accepts.

## FILES

Own:
- `teammates/orchestrator.md`, `teammates/opus.md`, `teammates/sonnet.md`,
  and the 8 Claude specialists: `architect-reviewer.md`,
  `backend-developer.md`, `designer.md`, `frontend-developer.md`,
  `product-lead.md`, `qa-engineer.md`, `researcher.md`, `staff-engineer.md`
- `teammates/codex-sol.md`, `teammates/codex-terra.md`,
  `teammates/orchestrator-codex.md` (only if Step 2 finds a Codex switch)
- `teammates/opencode-*.md` (only if Step 2 finds an OpenCode switch)
- `teammates/_template.md`, `teammates/README.md`
- `crates/horch-core/src/teammates.rs` (roster check rule + tests)
- `crates/horch-core/src/launch.rs` (only if a new field must reach the
  command line; say so in a `horch note` first)
- `crates/horch-core/tests/golden/*` only if a golden captures a command
  line that changes; follow the file's sanctioned-difference convention,
  do not add a refresh path
- `README.md` section near line 150 ("A fresh `claude` inherits...")
- `ai_docs/reports/no-subagents.md` (new)

Do NOT touch: `teammates/_base/*` (the prose rule is already there after
the bake-in merge), `skills/`, `docs/phase-skills.md`, `.herdr-orchestrator/`.

## STEPS

### Step 1. Verify the Claude switch empirically

Run from the repo root and paste the outputs into
`ai_docs/reports/no-subagents.md`:

```
claude --disallowedTools Agent -p 'Use the Agent tool to list the files in this directory. If you cannot, say exactly: AGENT TOOL UNAVAILABLE.' --max-turns 2
claude -p 'Use the Agent tool to list the files in this directory. If you cannot, say exactly: AGENT TOOL UNAVAILABLE.' --max-turns 2
```

Also test the tool name `Task` the same way; older Claude Code versions
used it. Record which names exist on this version (`claude --version`).
Then test `CLAUDE_CODE_FORK_SUBAGENT=false` if `claude --help` or the
env-research report documents it. Record what it switches off.

Decision: the deny list is every subagent tool name that exists on this
version. Expect `Agent`. Add `Task` only if it exists.

### Step 2. Find the switch on the other harnesses

For each of Codex, OpenCode, Prime, pi: read its `--help` and config docs
(`codex --help`, `codex config --help` or its config reference; `opencode
--help`; the Prime and pi launch paths in `crates/horch-core/`). Find any
feature that starts a nested agent: Codex multi-agent or collab features,
the OpenCode `task` tool, anything in Prime or pi. For each, record in the
report: the feature, the off switch (a `-c key=value`, a config JSON key,
or an env var), and how a teammate file passes it (`args:`, `env:`, or the
harness config field horch already forwards). If a harness has no such
feature, record "none" with the evidence.

Do not guess. A switch you cannot verify goes in the report as "unverified,
not applied".

### Step 3. Apply the switches in the teammate files

For every `agent: claude` teammate that is offered or is the orchestrator
(the 11 files in FILES): add `disallowed_tools: [Agent]` (plus `Task` if
Step 1 found it) and, if verified, `env: { CLAUDE_CODE_FORK_SUBAGENT: "false" }`.
Add a one-line comment above: `# Fleet rule: no subagents. Ask the
orchestrator for more workers.` Merge into an existing `disallowed_tools`
or `env` block if the file already has one.

For each other harness with a verified switch from Step 2: apply it in the
matching teammate files through the field horch already forwards.

Do not touch `smoke.md`, `orchestration-orchestrator.md`, or
`orchestration-worker.md`.

Check: `horch teammates --check` passes; `horch teammates --json` shows the
deny list on all 11 Claude teammates.

### Step 4. Enforce in the roster check

In `teammates.rs` `check()`: add a rule. For every teammate with
`agent: claude` that is not hidden, or that is the `orchestrator`, if
`disallowed_tools` does not contain `Agent`, push
`"{who}: a fleet pane must not spawn subagents; add disallowed_tools: [Agent]
or set allow_subagents: true"`.

Add the field `allow_subagents: bool` (default false) to the teammate
struct, parsed from frontmatter, documented in `_template.md` next to
`disallowed_tools`. When true, the rule is skipped. No shipped teammate
sets it.

Unit tests in `teammates.rs`: a Claude teammate without the deny fails the
check with that message; with the deny passes; with `allow_subagents: true`
passes; a codex teammate is not affected.

Check: `cargo test -p horch-core teammates` passes; `horch teammates --check`
passes on the shipped roster.

### Step 5. Docs, verify, PR

- `teammates/README.md` "Rules": one bullet on the no-subagents rule, the
  field, and `allow_subagents`.
- `README.md` near line 150: one paragraph on the fleet rule and how it is
  enforced per harness, from the Step 2 table.

```
cargo build --release --bin horch
cargo test --workspace
horch teammates --check
horch smoke fleet
cargo fmt --check -p horch-core -- crates/horch-core/src/teammates.rs
```

You are already on branch `no-subagents` in the worktree (see CONTEXT).
First commit line:
`Fleet panes cannot spawn subagents; roster check enforces it`. Push, then
`gh pr create --base bake-in-orchestration`. PR body: the Step 1 outputs,
the Step 2 table, the test count, and one line: "Stacked on PR 5; retarget
to main after it merges."

## CONSTRAINTS

- Verify every switch before you apply it. Unverified switches go in the
  report only.
- Do not edit `teammates/_base/*`.
- No subagents, no Agent tool, no background tasks in your own session.
- Follow the golden test file's sanctioned-difference convention if a
  golden changes; do not add a refresh path.

## DONE WHEN

- `ai_docs/reports/no-subagents.md` has the Claude evidence and the
  per-harness table.
- All 11 Claude teammates carry the deny list; `horch teammates --check`
  passes and fails a test teammate without it.
- `cargo test --workspace` and `horch smoke fleet` pass.
- PR open against `main`.

## REPORT

- `horch note "..."` after each step.
- `horch tell orchestrator "[<role>] BLOCKED: ..."` if stuck, then wait.
- `horch done "..."`: PR URL, branch, commit, the per-harness table in 4
  lines, files changed, test count.
