# teammates/

One file per team member. **Adding a file adds an option the orchestrator can
choose from** — there is no registry to update and nothing to recompile.

> **These files are live.** They are the only place agent briefings exist.
> `crates/horch-core/src/prompts.rs` holds no prompt text; it reads these files
> and substitutes `{placeholder}` spans, nothing more. `tiers.rs` is gone.
>
> ```bash
> horch teammates            # who the orchestrator can pick
> horch teammates --check    # validate before a fleet reads them for real
> horch teammates --new qa   # scaffold qa.md from _template.md
> ```
>
> Edits take effect on the next `just herdr-fleet` with no rebuild, because the
> justfile exports `HORCH_TEAMMATES_DIR` and the roster is read at run time. A
> copy is also compiled in, so an installed `horch` binary works with no repo
> checked out.

## Rules

- **Filename is the id.** `opus.md` → `horch spawn opus`. `name:` in the
  frontmatter must match, and a mismatch is a load-time error rather than a
  quietly-wrong roster.
- **A leading `_` means "not parsed."** `_template.md` and `_base/` are not
  teammates and are never read as such.
- **`hidden: true` means "loaded, but not offered."** The teammate is spawnable
  by name and never appears in the roster. `smoke.md` uses this, so
  `horch smoke fleet` keeps working without costing the orchestrator a roster
  line for a fake agent.
- **`brief_description` is the only field the orchestrator reads** to decide
  between a specialist and a generic. One line, ≤120 characters, enforced at
  load. Everything else — persona, model, effort, skills — is applied when the
  teammate is spawned and is never in the orchestrator's context.
- **Be explicit about mode and tools.** Anything left unset is inherited from
  the operator's `~/.claude/settings.json`, which is written for an attended
  session, not for a worker in a pane nobody is watching. On this machine that
  file denies `ExitPlanMode`, so a plan-mode teammate that says nothing about
  tools cannot leave plan mode. See the `permission_mode` / `tools` block in
  `_template.md`.
- **Personas do not carry protocol.** `_base/fleet-worker.md` owns the
  `horch tell` / `horch note` / `horch done` contract; a persona that restates
  it will drift from it.

## The team

`product-lead`, `researcher`, `staff-engineer`, `designer`,
`architect-reviewer`, `frontend-developer`, `backend-developer` and
`qa-engineer` are the specialists. Each loads only the plugins, skills and MCP
servers its job needs, and nothing else: they set `inherit_plugins: false`,
which switches the operator's globally-enabled plugins off by name for that
session while leaving every other setting exactly as tuned.

Plugin paths are written with a leading `~/` and expanded at launch, so these
files are not bound to one home directory. `horch teammates --check` verifies
every path exists before a fleet tries to use it.

## The five generics

`sonnet`, `opus`, `codex-sol`, `codex-terra` are the fallbacks: what
the orchestrator spawns when no specialist's `brief_description` matches the
work. They are named after the current tiers on purpose, so `horch spawn opus`,
the ledger's `tier` field, and the tier table in the orchestrator briefing all
keep meaning the same thing.

## Adding a specialist

Copy `_template.md`, rename it, drop `generic:`, and write a
`brief_description` that answers "when would I reach for this?" — the
orchestrator is choosing from that line alone.
