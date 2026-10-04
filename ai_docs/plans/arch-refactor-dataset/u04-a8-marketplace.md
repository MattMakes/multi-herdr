# U04 a8-marketplace: the horch-marketplace crate (internal APIs and tests)

Unit slug: `a8-marketplace`. Branch: `ard/a8-marketplace`. Phase: A8.
Requirements: MKT-01 to MKT-08 (A8 parts), MKT-10, NFR-07.

## GOAL

A new library crate `crates/horch-marketplace` installs a skill from a
bundled, local or git source through a transactional, verified pipeline and
records it in a lock file. It has no dependency on `horch-core`. All MKT
tests of phase A8 pass. No CLI is added (that is A11).

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Operator decisions" (OD3, OD6), "Hard
  constraints", §1 (target layout, dependency direction), §3 "A8", §4 "MKT"
  and NFR-06/NFR-07 rows.
- SKILL.md validation rules today: `crates/horch-core/src/skills.rs`, function
  `catalog()` (about lines 39 to 73). A skill dir has `SKILL.md` with YAML
  frontmatter `name` and `description`. `name` equals the dir name, is
  lowercase ASCII letters, digits and `-`, does not start or end with `-`,
  has no `--`. `description` is non-empty and at most 1024 bytes. Copy these
  rules into the marketplace crate (do not import core).
- Bundled skills live in the repo `skills/` dir; `skills/provenance.json`
  pins upstream `MattMakes/skill-marketplace` at commit
  `d47670328c59a3311a9b4149bc5f8f33f0a92754` with a per-skill
  `source_sha256`. Phase A9 (later) merges bundled entries and lock entries
  into one catalog in core. Your crate defines the `Bundled` source type only;
  it does not embed the bundled files.
- Spec A §10 text is not available. Mark the manifest shape
  `SPEC-RESOLVED(Spec A §10)`. (closed: ai_docs/designs/2026-10-02-architecture-refactor-design.md §4.10/§6, ai_docs/reports/finish/spec-a-marketplace.md)
- U01 adds the `horch-marketplace` allowlist to `scripts/check-deps.sh` and
  `nfr_06` in `crates/horch-core/tests/nfr.rs`. Before READY-TO-MERGE, rebase
  onto `arch-refactor-dataset` after U01 merged. If U01 has not merged when
  you finish, send `NOTE:` and wait.
- Later, `horch_core::vcs::git` (phase B2) wraps your low-level git runner
  `horch_marketplace::git`. Make that module general (run any git argv in a
  dir, with a controlled env) and public.

## FILES

own:
- `crates/horch-marketplace/**` (new crate)
- `Cargo.toml` (root: add the member and the workspace dependency
  `horch-marketplace = { path = "crates/horch-marketplace" }`)
- `Cargo.lock`
- `ai_docs/reports/arch-refactor-dataset/a8-marketplace.md`

do not touch: `crates/horch-core/**`, `crates/horch/**`, `crates/horch-e2e/**`,
`scripts/**`, `justfile`.

## DESIGN (implement this)

Modules, all in `crates/horch-marketplace/src/`:
`lib.rs model.rs catalog.rs manifest.rs source.rs resolver.rs store.rs
installer.rs lockfile.rs integrity.rs git.rs fsx.rs error.rs`.

- `Cargo.toml` dependencies: `anyhow`, `serde`, `serde_json`, `serde_yaml`,
  `sha2` (version `0.10`, already in `Cargo.lock`). Dev: `tempfile`. Nothing else.
- `model.rs`:
  - `SkillId` (validated with the SKILL.md name rules), `SkillVersion(String)`.
  - `enum SkillSource { Bundled { name }, Local { path }, Git { url, revision: GitRevision, subdir: Option<String> } }`.
  - `enum GitRevision { Branch(String), Tag(String), Commit(String) }`.
  - Parse a source spec string: `owner/repo[@rev]` → GitHub https URL;
    `https://...[@rev]`, `file://...`, or an absolute local path. `@rev` with
    40 hex chars is a `Commit`; otherwise try tag, then branch, during resolve.
- `manifest.rs`: `SkillManifest` parsed from the SKILL.md frontmatter with
  `#[serde(deny_unknown_fields)]`. Allowed keys: `name`, `description`,
  `license`, `metadata` (a string map), `allowed-tools`. Any other key (for
  example `hooks`) is rejected. Mark the key list `SPEC-RESOLVED(Spec A §10)`. (closed: ai_docs/designs/2026-10-02-architecture-refactor-design.md §4.10/§6, ai_docs/reports/finish/spec-a-marketplace.md)
- `git.rs`: `pub struct GitRunner { bin: PathBuf }` and
  `pub fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError>`.
  The child env: clear nothing globally, but always set
  `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`, `GIT_CONFIG_NOSYSTEM=1`,
  `core.hooksPath=/dev/null` through `-c`, and remove `ANTHROPIC_API_KEY`.
  The bin path is a constructor parameter (the caller resolves `HORCH_GIT_BIN`
  later; this crate never reads env vars).
- `resolver.rs`: `resolve(&SkillSource) -> ResolvedSource` with a full commit
  SHA for git (`git ls-remote` for branch/tag; verify a commit by fetch).
- Fetch (in `source.rs` or `resolver.rs`): bare clone or fetch into
  `<store>/staging/<uuid-like random hex>/`, then check out the resolved
  commit's tree into a staging dir (`git archive` or `worktree`-free
  `read-tree`/`checkout-index`). Hooks are off.
- `integrity.rs`: walk the staged skill dir. Reject: absolute paths, `..`
  components, symlinks, more than 512 files, any file over 1 MiB, total over
  8 MiB. Compute the tree digest: sha256 over the sorted list of
  `<relative path>\0<sha256 of bytes>\n`. Display as `sha256:<hex>`.
- `store.rs`: store root is a constructor parameter (the caller passes
  `${XDG_DATA_HOME:-~/.local/share}/horch/`). Layout: `staging/`,
  `skills/<id>/<version>/`, `marketplace.lock`. Version string:
  `git+<commit12>` for git, `local+<digest12>` for local,
  `bundled+<digest12>` for bundled.
- `lockfile.rs`: `marketplace.lock` JSON `{ "version": 1, "skills": [ ... ] }`
  sorted by id. Entry fields exactly: `id`, `source`, `requested_revision`,
  `resolved_commit`, `version`, `digest`, `installed_at`. Atomic replace:
  write a temp file in the same dir, fsync, rename, fsync the dir.
- `installer.rs`: `install(&self, source, opts: &InstallOptions) -> Result<LockEntry>`.
  Order: catalog → resolve → fetch → validate (manifest + SKILL.md rules) →
  verify (integrity, digest) → materialize (atomic rename of the staged dir
  into `skills/<id>/<version>/`) → lock. `InstallOptions` has
  `fault: Option<FaultPoint>` where `FaultPoint::AbortAfterMaterializeBeforeLock`
  returns an error at that point (the caller maps `HORCH_FAULT` to it later).
  On any error, staging is removed; an already materialized version dir that
  has no lock entry is ignored by readers and replaced by the next install.
  Record each pipeline step in an optional observer (a `&mut Vec<Step>`) so a
  test can assert the order.
- Offline reinstall: `reinstall_from_lock(&self)` rebuilds every locked skill
  from its pinned commit when `skills/<id>/<version>/` is missing, and needs
  no network when the version dir exists (it only verifies the digest).
- Credentials: reject any URL with userinfo (`https://user:pass@host`,
  `https://token@host`) and any `?` query with `token=` before running git.
- `error.rs`: one hand-written `MarketplaceError` enum with `Display` and
  `std::error::Error`. No `thiserror`.

## STEPS

1. Create the worktree (conventions §2). Add the crate and the root member.
   Check: `cargo build -p horch-marketplace`.
2. Implement the modules above, one commit per 2 or 3 modules, each gated.
3. Tests in `crates/horch-marketplace/tests/marketplace.rs`. Each git test
   builds a local bare repo fixture in a tempdir with real git (set
   `GIT_CONFIG_GLOBAL` to an empty temp file and `GIT_CONFIG_NOSYSTEM=1` on
   the fixture commands). If `git` is not on PATH, skip the test unless
   `HORCH_REQUIRE_GIT=1`, in which case fail. Tests:
   - `mkt_01_no_core_dependency`: parse this crate's `Cargo.toml`; assert no
     `horch-core` dependency; scan `src/` for `horch_core` and `teammate`
     (case-insensitive) and assert none.
   - `mkt_02_branch_resolves_to_sha`: branch and tag both resolve to the full
     40-hex commit; moving the branch later does not change an installed entry.
   - `mkt_03_lifecycle_order`: the observer records exactly
     catalog, resolve, fetch, validate, verify, materialize, lock.
   - `mkt_04_lock_entry_fields`: the lock JSON entry has exactly the 7 fields.
   - `mkt_05_install_is_transactional`: with the fault, no lock entry exists,
     staging is empty, and a second install without the fault succeeds.
   - `mkt_06_rejects_traversal`, `mkt_06_rejects_symlink`,
     `mkt_06_rejects_oversize`, `mkt_06_rejects_invalid_skill_md`,
     `mkt_06_rejects_hooks` (a `hooks:` frontmatter key).
   - `mkt_07_rejects_credentials_in_url`.
   - `mkt_08_offline_reinstall_from_lock`: install, delete the fixture repo,
     then reinstall from lock succeeds when the version dir exists and
     verifies the digest; with a tampered file it fails.
   - `mkt_10_source_dispatch_localized`: scan `src/`; `SkillSource::` match
     arms appear only in `resolver.rs` and the fetch module.
   - `nfr_07_git_only_on_temp_repos`: scan `tests/` and `src/`; every git
     invocation in tests uses a tempdir path (assert the helper is the only
     place that builds fixture repos), and `GitRunner` never runs without an
     explicit dir.
4. Run the gate. Rebase on the integration branch after U01 merged. Run the
   gate again; `scripts/check-deps.sh` must pass with the crate present.
5. Write and commit the report. It must describe the public API that A9 and
   A11 will call, and the `git` module API that B2 will wrap.
6. Follow conventions §6 to finish.

## DONE WHEN

- `cargo test -p horch-marketplace` passes with `HORCH_REQUIRE_GIT=1`.
- `./scripts/check-req-coverage.sh MKT-01 MKT-02 MKT-03 MKT-04 MKT-05 MKT-06 MKT-07 MKT-10 NFR-07`
  passes after U03's design docs merge (until then, the test names match the
  master plan exactly).
- `scripts/check-deps.sh` passes. The full gate is green.

## REPORT

- `horch note` after each 2 or 3 modules.
- `horch done` summary: public API, store layout, lock format, SPEC-RESOLVEDs. (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)
