# K1 docs-refresh report

Branch `ds/docs-refresh`. Date 2026-10-04. Plan: `ai_docs/plans/finish/k1-docs-refresh.md`.

## Inventory: feature, where documented before, gap, fix

| feature | documented before | gap | now |
|---|---|---|---|
| vendored skills, re-vendoring | `skills/README.md` only | no handbook page; `add-skill.md` cited removed `DESIGN_SOURCE_PINS` and had no vendor steps | `docs/skills-and-teams.md` (kinds, rules, re-vendor steps); `add-skill.md` fixed and has "Vendor an upstream skill" |
| `available_skills`, `operator_skills` | `add-teammate.md`, `teammates/README.md` | `add-teammate.md` said the ledger does not list operator skills (wrong since V3) | corrected; page explains precedence, cost and `--check` rules |
| `offer_when`, `requires` | `add-teammate.md`, `teammates/README.md` | no rules for the glob match or the filter scope | `docs/skills-and-teams.md` |
| `horch doctor` checks | README 1 line ("herdr reachable") | roster, `requires`, effort overrides not listed | README paragraph; table in the new page; `command-flow.md` node |
| Unreal team, Git LFS, UE 5.8 | `teammates/README.md` | no handbook how-to | summary and rules in the new page, link to `git-lfs.md` |
| Swift team, MobileBuildMCP, app-release-preparer | `teammates/README.md` | no handbook how-to | summary in the new page; the Apple section of `README.md` is not edited (opus-56) |
| Blender teammate | `teammates/README.md` | no usage page | section "Blender and the Unreal team" in the new page |
| design team | `teammates/README.md` (members) | no usage page | summary in the new page |
| `horch agent-list` | README, `command-flow.md` | no output description | section in the new page |
| Antigravity harness | README table, `add-harness.md` | no usage facts in 1 place | section in the new page |
| `multi-herdr-dataset` | README, design, recipe | no command list | section in the new page |
| gate: clippy, git-env scrub, never `rebase -x`, slots | none (`testing-and-gates.md` had no clippy step) | gate table was 8 steps, real gate has 9 | table fixed; warning, slot wrapper, symlink-fake note added |

## Files

- new `docs/skills-and-teams.md`
- changed `docs/README.md`, `docs/testing-and-gates.md`, `docs/command-flow.md`, `docs/recipes/add-skill.md`, `docs/recipes/add-teammate.md`, `README.md` (3 places: doctor, `just gate`, a pointer section)

## Verification

Run with the installed `horch` (0.1.0) and `multi-herdr-dataset`:
`horch --help`, `skills --help`, `skills show ue-build-verify` (prints `vendored: true`), `skills show skill-creator`, `teammates --help`, `--check`, `--matrix --json`, `agent-list --no-probe`, `agent-list --json --no-probe`, `route --help`, `cost --help`, `doctor --help`, and `multi-herdr-dataset --help`, `run --help`, `readiness --help`. `just --list` shows `gate`. `scripts/phase-gate.sh` read for the 9 steps. Relative links in the 6 changed pages resolve. `horch teammates --check` with `HORCH_TEAMMATES_DIR=teammates`: roster ok. Full gate not run (markdown only; merge-train rule).

I did not run `horch doctor` itself (needs a herdr server); its checks come from `crates/horch/src/cmd/doctor.rs`.

## Not done, outside scope

- The `install-checks` unit is not merged. When it merges, update the `horch doctor` table in `docs/skills-and-teams.md`.
- After the rebase: `docs/recipes/add-teammate.md` keeps the operator-skills text of ds/operator-skills; `docs/skills-and-teams.md` follows its warning behaviour.
- `gate-slot.sh` lives in the git-ignored `.worktrees/`, so a fresh clone lacks it. Decide whether to move it to `scripts/`.
- `docs/phase-skills.md` was not re-checked against the 33 `ue-*` and 28 Swift skills.
