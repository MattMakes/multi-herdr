# A model is named by family, and the newest version is chosen at launch

A teammate file, a fleet flavor and a context-window key name a model by
its family: `opus`, `sonnet`, `sol`, `terra`, `luna`, `astra`. No version
is written in the roster or in the code that selects a model, so a new
version of a family is launched as soon as the harness lists it, with no
edit and no release of horch.

Claude Code resolves `opus` and its other aliases itself. The codex CLI
takes only a slug with a version, so `harness::codex::resolve_model` does
the same job for Codex: it reads `codex debug models` at each launch and
takes the highest version whose family matches. `harness::models` holds the
rule that splits a model id into a family and a version, and every place
that must treat versions of a family alike uses it.

## Consequences

- `gpt-5.6-sol`, `gpt-6-astra` and `claude-opus-5-5` all split the same way:
  the first word is the vendor, the numbers are the version, the other
  words are the family. A name with no digit is a family.
- A slug with a version is launched as written, and `codex debug models` is
  not asked. This is how the operator pins a version: in a teammate file,
  or with `horch spawn --model`.
- A model that the catalog marks `visibility: hide` or `none` is never
  chosen for a family, so an unreleased model is not launched because it
  has the highest number. A name that is itself a slug in the catalog is
  used as written.
- The ledger record keeps the family name, as it keeps `opus`. A resumed
  session resolves the family again, so it can move to a newer version,
  which is what `--model opus` already does on a Claude resume.
- Each successful lookup is saved in `<state>/codex/model-families.json`.
  When `codex debug models` fails, the family stays on the saved slug and
  the launch prints a note. With no saved slug the launch fails and names
  `horch spawn --model`, because a guessed version would start a pane that
  fails at its first turn.
- `compaction::window::fleet_window` tries `<harness>/<family>` after the
  teammate and the exact model, so `codex/sol` in `_base/context-windows.md`
  serves every Sol version.
- `usage::price_for` prices a family name as the newest row of that family
  in the price table, so `MODEL_ALIASES` is gone and a new version needs 1
  price row. A session that names a version with no price row stays
  unpriced ([0147](0147-horch-cost-is-informational-and-never-prices-unknowns-as-zero.md)):
  a price is a fact that horch cannot derive.
- `flavor_of_model` compares families, so a respawn keeps the flavor of an
  orchestrator whose record names any version.
- Antigravity, pi, Prime and opencode teammates still name a model with its
  version. horch has no verified way to list their models yet.
