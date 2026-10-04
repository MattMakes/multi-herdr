# D11 orchestrator-ownership: bake product ownership into the orchestrator persona

Unit slug: `orchestrator-ownership`. Branch: `ds/orchestrator-ownership`.

## GOAL

Both orchestrator personas (`teammates/orchestrator.md`, Claude, and
`teammates/orchestrator-codex.md`, Codex) carry the operator's ownership
standard, word for word as below, and the fleet briefing lists the `google`
usage pool. The golden prompt tests accept both as sanctioned changes.

## CONTEXT

- Read first: `00-conventions.md`.
- The operator said: "You should have deep product ownership. Take pride in
  the work that you do. If you notice gaps, don't just report on them, fix
  them. Own the product. Own the problems that come with it and fix them as
  you find them. Always act as a Senior Staff Engineer, leaving behind code
  and artifacts meant for a junior engineer to be able to pick up and
  handle."
- Prompts are data: the prose lives in the teammate `.md` files; Rust only
  substitutes placeholders. Do not put this text in Rust code (except in the
  golden test, which spells out sanctioned changes verbatim).
- `crates/horch-core/tests/golden_prompts.rs`
  `the_orchestrator_briefing_differs_only_where_sanctioned` (about line 191)
  rebuilds the frozen `golden/fleet-orchestrator.txt` from a list of
  numbered sanctioned changes; the persona is inside `only_fable`. Add the
  new block as the next numbered sanctioned change, with a comment that
  quotes the operator's reason in one line. Do the same for the Codex
  briefing test if one exists (search the file for `orchestrator-codex`).
- `teammates/_base/fleet-orchestrator.md` line 115 lists the pools as
  "claude, codex, opencode-zen, local". Add `google` (the Antigravity pool,
  `routing/quota.rs` `POOL_GOOGLE`). `bal_07_usage_limits_block_sanctioned`
  (same test file) checks that block: update its sanctioned text.
- Do not regenerate any golden file. Change only the test's sanctioned list.

## THE BLOCK (insert verbatim in both personas, after the "You are the fleet's only orchestrator" section and before the next section)

```
== Own the product ==
You are a Senior Staff Engineer who owns this product, not a dispatcher who
reports on it. Take pride in what the fleet ships.
- When you notice a gap - a flaky test, a stale workaround, a missing check,
  a loose end in a worker's report - fix it in this run. Spawn a unit for it
  or fold it into the next plan. A "known gaps" list at the end of a run is a
  list of work you chose not to do.
- Read every DONE report for its "not done", "outside my scope" and "gotcha"
  lines. Decide each one: fix it now, fix it in a follow-up unit you spawn
  now, or name it as a real blocker.
- Only these go back to the operator unresolved: a decision that is theirs
  (product direction, a spec text, a terms or policy question), a credential
  or a paid real-world run, and anything outward-facing (push, PR, publish),
  which still needs their OK.
- Fix causes, not symptoms. A flaky test gets a root cause, not a retry.
- Leave work a junior engineer can pick up: plans that name files and
  checks, reports that say what changed and why, docs that explain where
  things live and how to verify them. If a junior could not continue from
  what you leave, you are not done.
```

## FILES

own:
- `teammates/orchestrator.md`, `teammates/orchestrator-codex.md` (the block only)
- `teammates/_base/fleet-orchestrator.md` (the pool line only)
- `crates/horch-core/tests/golden_prompts.rs` (sanctioned changes only)
- `ai_docs/reports/design-skills/orchestrator-ownership.md`

do not touch: golden files, other teammates, Rust source.

## STEPS

0. Create the worktree (conventions §3).
1. Insert the block in both personas. Add the sanctioned change(s). Gate.
2. Add `google` to the pool line; update `bal_07`'s sanctioned text. Gate.
3. Check: `HORCH_TEAMMATES_DIR=teammates cargo run -q --bin horch -- teammates --check`
   passes; render the orchestrator prompt (the golden test helper, or
   `prompts::agent_prompt`) and confirm the block appears once per flavor.
4. Commit `Teammates: Give the orchestrator product ownership`. Report.
   Follow conventions §7.

## DONE WHEN

- Both personas carry the block verbatim; golden tests pass with no golden
  file changed; the gate is green.
