# A11 a11-marketplace-cli: horch skills / horch marketplace commands and just recipes

Branch `ard/a11-marketplace-cli`. Phase A11. Requirements MKT-08 (A11
parts), MKT-09. Base: `arch-refactor-dataset` at 15210a7.

## Result

- `horch skills`, `horch skills --phase <p>`, `horch skills --json` and
  `horch skills --phase <p> --json` print the A0 oracle output byte for
  byte. `horch skills list ...` is an alias. `oracle_cli_skills_match`
  passes unchanged.
- New commands: `horch skills show|install|update|doctor` and
  `horch marketplace list|refresh`.
- `MaterializedSkills::materialize` copies a marketplace skill from the
  store instead of refusing it.
- `just skills`, `skills-install`, `skills-update`, `skills-doctor` and
  `marketplace-refresh` exist. `just skills` prints the legacy output.
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`.
  `./scripts/check-req-coverage.sh --phase A11` exits 0. MKT-08 under
  `--phase A8` lists all 3 tests.

## Commits

1. `A11: Move horch skills into a subcommand group`
2. `A11: Add skills show/install/update/doctor and marketplace commands`
3. `A11: Materialize marketplace skills from the store`
4. `A11: Add just recipes`
5. `A11: Add MKT-08 and MKT-09 tests` (also holds 2 security-review fixes)
6. This report.

## Commands

The store root is `ctx.paths.data_root` =
`${XDG_DATA_HOME:-$HOME/.local/share}/horch/` (OD3). Git is
`ctx.bins.harness.git` (`HORCH_GIT_BIN`, else `git`).

| Command | Does | Output |
|---|---|---|
| `horch skills [list] [--phase P] [--json]` | legacy `describe(phase)` | A0 oracle text or JSON |
| `horch skills show <id> [--json]` | `SkillCatalog::installed(data_root)` lookup | JSON keys: `id version source digest description provenance install_path` |
| `horch skills install <source>[@rev] [--path <subdir>]` | `parse_source`, `with_subdir`, `Installer::install` | `installed <id> <version> (commit <40 hex>)`, or `unchanged ...` when the lock already had that version and digest |
| `horch skills update [<id>]` | `Installer::update` | `<id> <old> -> <new>` per skill |
| `horch skills doctor` | `catalog::check_store`, no git | `ok/missing/changed/invalid` per entry; exit 1 on any problem |
| `horch marketplace list [--json]` | `check_store` | JSON `{"store", "skills": [{id, source, requested_revision, resolved_commit, version, digest, installed_at, path, present}]}` |
| `horch marketplace refresh` | `Installer::reinstall_from_lock` | `ok <id> <version>` lines, then a count |

- `show`: `source` is `CatalogEntry::source_label()` (`bundled`,
  `<spec>@<commit>` or `<spec>`). `install_path` is `null` for a
  compiled-in skill. `provenance` is `null` for a marketplace skill.
- `install` honors `HORCH_FAULT=abort-after-materialize-before-lock`
  through `ctx.settings.faults`.
- `bundled:<id>` installs work: the installer gets the compiled-in files
  through `SkillCatalog::marketplace_catalog()`.

## Core API added (`crates/horch-core/src/skills/catalog.rs`)

```rust
pub use horch_marketplace as marketplace;          // skills::catalog::marketplace (orchestrator option 1)
SkillCatalog::installed(data_root) -> Result<SkillCatalog>   // bundled + lock; descriptions from SKILL.md
catalog.store_dir(&entry) -> Result<Option<PathBuf>>         // <data_root>/skills/<id>/<version>/, validated
catalog.marketplace_catalog() -> Result<marketplace::Catalog>
catalog::installer(data_root, git_bin) -> Result<Installer>
catalog::check_store(data_root) -> Result<Vec<(LockEntry, PathBuf, StoreState)>>
StoreState { Ok, Missing, Tampered { actual }, Invalid(String) }
```

`SkillCatalog` has a new private field `store_root: Option<PathBuf>`.
`bundled()` and `with_lock()` leave it `None`; only `installed()` sets it.
A marketplace entry in a catalog without a store root still fails to
materialize, with a clear message.

`materialize` for a marketplace entry: `store_dir` validates the id
(`SkillId::parse`) and the version (not empty, not `.`/`..`, no `/ \ NUL
:`). It copies regular files and directories only (a symlink or special
file fails). Then it computes the tree digest of the copy and requires the
locked digest. So launch reads exactly what the lock pinned.

## Decisions

- The binary reaches the marketplace through the core re-export, because
  `check-deps.sh` and `nfr_05` forbid a direct dependency (orchestrator
  approved option 1).
- `mkt_08_runtime_needs_no_network` is in `crates/horch/tests/skills_cli.rs`,
  not in `horch-e2e`, because `horch-e2e` has no `horch-core` dependency.
  It installs a git skill through the CLI, then runs `skills doctor` and
  `marketplace refresh` with `HORCH_GIT_BIN=/nonexistent`, then calls
  `SkillCatalog::installed`, `plan_activation` and
  `MaterializedSkills::materialize` (orchestrator option 3).
- A repeat install is a no-op on pin, digest and the version directory.
  The marketplace installer still rewrites `installed_at`. The CLI prints
  `unchanged`. I did not change `horch-marketplace`.
- The legacy `horch skills` listing shows bundled skills only. It must stay
  equal to the A0 oracle. Use `horch marketplace list` or `skills show`
  for installed skills.
- `SkillCatalog::installed` fails when a lock entry has an invalid id or
  version. That is loud on purpose: the lock is input.

## Tests (4 added)

- `crates/horch/tests/skills_cli.rs`:
  `mkt_09_legacy_skills_flags_output_unchanged` (4 legacy forms, and the
  same 4 through `list`, all 10 oracle files), `mkt_09_cli_list_show_json`
  (keys of `show --json` and `marketplace list --json`, empty and with a
  local install; also the 2 security assertions below),
  `mkt_08_runtime_needs_no_network`.
- `crates/horch-e2e/tests/marketplace.rs`:
  `mkt_08_e2e_install_update_repeatable` (`Harness::with_git()`, a bare
  fixture repo with `skills/demo`; pin is 40 hex, repeat is a no-op,
  moved branch plus `update` changes the version, `doctor` passes with
  `HORCH_GIT_BIN=/nonexistent`, a changed file makes `doctor` exit 1).
- Red check: with the pre-A11 `materialize.rs`, `mkt_08_runtime_needs_no_network`
  fails with "comes from the marketplace store, which launch cannot read
  yet". The other tests were written after the code; they guard the
  contract.

## Security review (horch:security-review, focused)

Scope: the A11 diff (`skillscmd.rs`, `marketplacecmd.rs`, `catalog.rs`,
`materialize.rs`). Trust boundaries: the source spec (operator argument),
`marketplace.lock` and the store (operator-writable files), and SKILL.md
text from a third-party git repository. Source review only; no scanner.

- CONFIRMED, fixed, low: `install` errors echoed the source spec. A spec
  with userinfo or a token would reach stderr, although the marketplace
  error itself hides it. The message is now `install: <error>`. Test:
  `mkt_09_cli_list_show_json` (`https://user:hunter2@...` is not echoed).
- CONFIRMED, fixed, low: `skills show` printed the SKILL.md description
  raw. A YAML `"\e[..."` escape reaches the terminal. Control characters
  now print as `?` (text form only; JSON escapes them). Test:
  `mkt_09_cli_list_show_json`.
- DISMISSED: path traversal through lock `id`/`version`. `version_dir`
  validates both before any join. `:` is also rejected (Windows drive
  prefix).
- DISMISSED: a store file changed after install reaching a launch. The
  copy's digest must equal the lock. Test: `mkt_08_runtime_needs_no_network`.
- ACCEPTED, low: if `skills/<id>/<version>` itself is a symlink,
  `read_dir` follows it. Files below it are still regular-file checked,
  and the digest must still match the lock. Writing the store needs the
  same access as writing the lock.
- NOT IN SCOPE: SKILL.md bodies are prompt text from a third party. This
  is inherent in installing a skill; A10 exposes it to harnesses.

## For A10 (orchestrator instruction)

A10 must do 2 things:
1. Roster validation must accept marketplace ids.
   `roster/validation.rs:46` calls `skills::selected`, which checks names
   against `SkillCatalog::bundled()` only (`selection.rs:32`). A teammate
   whose `skills:` lists an installed id fails at roster load today.
2. `cmd/worker.rs:113` and `cmd/recipes.rs:609` must pass
   `ctx.paths.data_root`, so `Bundle::install` (in `skills.rs`) builds its
   catalog with `SkillCatalog::installed(data_root)` instead of
   `bundled()`. An e2e spawn must then check the no-network case
   (`HORCH_GIT_BIN=/nonexistent`), as LA-5 describes.

Other notes for A10:
- `ensure_supported` and `selected` also use the bundled catalog.
- The briefing gets marketplace descriptions only from a catalog built
  by `installed()`.
- `horch skills` (legacy) never lists marketplace skills.

## Outside my scope (noticed, not fixed)

- `horch-marketplace` has no inter-process lock on the store (A8 report).
  Two concurrent `skills install` calls can race on `marketplace.lock`.
- `cargo clippy` shows old warnings in `codex.rs`, `ledger.rs`,
  `measure/redact.rs` and `roster/operator.rs`. The gate does not run clippy.

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

None added.
