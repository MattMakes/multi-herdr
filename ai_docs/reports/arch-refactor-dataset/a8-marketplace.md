# A8 a8-marketplace: the horch-marketplace crate

Branch `ard/a8-marketplace`. Phase A8. Requirements MKT-01 to MKT-08 (A8
parts), MKT-10, NFR-07.

## What exists

`crates/horch-marketplace` is a library crate. It does not depend on
`horch-core` and does not read the process environment. Dependencies:
`serde`, `serde_json`, `serde_yaml`, `sha2 0.10`. Dev: `tempfile`.
`anyhow` is not used, so it is not declared; the public error type is
`MarketplaceError`.

Modules in `src/`: `lib model catalog manifest source resolver store
installer lockfile integrity git fsx error`.

## Public API for A9 (catalog) and A11 (CLI)

```rust
// Construction. The caller resolves every path and binary.
let store = Store::new(data_home.join("horch"));          // ${XDG_DATA_HOME:-~/.local/share}/horch/
let git = GitRunner::new(git_bin);                         // caller maps HORCH_GIT_BIN
let catalog = Catalog::new().with_bundled(BundledSkill { id, files: vec![BundledFile { path, bytes }] });
let installer = Installer::new(store, git, catalog);

// Sources.
SkillSource::parse("owner/repo@v1")?           // https://github.com/owner/repo.git
SkillSource::parse("https://host/r.git@<sha>")?
SkillSource::parse("file:///abs/repo.git@main")?
SkillSource::parse("/abs/skill-dir")?          // Local
SkillSource::parse("bundled:tdd")?             // Bundled
    .with_subdir("skills/tdd")                 // A11 `--path`; git only

// Install, list, offline reinstall.
installer.install(&source, &InstallOptions { fault: None })? -> LockEntry
installer.install_observed(&source, &opts, Some(&mut steps))?  // Vec<Step>
installer.installed()? -> Vec<InstalledSkill { entry, path }>  // lock entries whose dir exists
installer.reinstall_from_lock()? -> Vec<(SkillId, ReinstallAction::{Verified, Rebuilt})>
FaultPoint::parse("abort-after-materialize-before-lock")       // caller maps HORCH_FAULT
```

Revision rules: a 40-hex `@rev` is `GitRevision::Commit`. Any other `@rev`
parses to `GitRevision::Tag`, and resolve tries the peeled tag, the tag,
then a branch of that name. No `@rev` is `Branch("HEAD")`, the remote
default branch. The last `@` in a URL path starts the revision.

`SkillManifest::parse(text, expected_name)` and `SkillManifest::read(dir,
expected_name)` give A9 the SKILL.md rules (the rules of
`horch-core/src/skills.rs::catalog()`, plus the key allowlist). A9 can
build `BundledSkill` values from its compiled-in table (`bytes` is a
`Cow<'static, [u8]>`).

## Git module API for B2 (`horch_core::vcs::git`)

```rust
pub struct GitRunner { bin, env }
GitRunner::new(bin) / .with_env(key, value) / .bin()
fn output(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError>  // any exit status
fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError>     // non-zero exit = GitError::Failed
pub struct GitOutput { status, stdout: Vec<u8>, stderr: Vec<u8> }  // .stdout_text()
pub enum GitError { NoDir, Spawn { bin, source }, Failed { args, code, stderr } }
```

Every child: `current_dir(dir)` (an empty dir is `GitError::NoDir`), stdin
null, `-c core.hooksPath=/dev/null`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`,
`GIT_CONFIG_NOSYSTEM=1`. It removes `ANTHROPIC_API_KEY` and the
repo-locating variables `GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NAMESPACE
GIT_COMMON_DIR`. `with_env` variables are set first; the fixed ones win.

## Store layout

```
<root>/staging/<32 hex>/        one per install; removed on success and on every error
<root>/skills/<id>/<version>/   materialized skills
<root>/marketplace.lock
```

Version: `git+<commit12>`, `local+<digest12>`, `bundled+<digest12>`. A
version dir without a lock entry is ignored by `installed()` and replaced
by the next install of that version (moved into staging, then removed).
Older versions of a skill are not deleted after an update (no GC yet).

## Lock format

```json
{ "version": 1, "skills": [ {
    "id": "demo",
    "source": { "kind": "git", "url": "file:///…/remote.git",
                "revision": { "tag": "v1" }, "subdir": "skills/demo" },
    "requested_revision": "v1",
    "resolved_commit": "<40 hex>",
    "version": "git+<12 hex>",
    "digest": "sha256:<64 hex>",
    "installed_at": "2026-10-02T12:00:00Z" } ] }
```

Exactly 7 entry fields; `requested_revision` and `resolved_commit` are
`null` for local and bundled. Sorted by id, one entry per id,
`deny_unknown_fields`. Written by temp file, fsync, rename, dir fsync.
`source` kinds: `{"kind":"bundled","name"}`, `{"kind":"local","path"}`,
`{"kind":"git","url","revision":{"branch"|"tag"|"commit":…},"subdir"}`.

## Pipeline and checks

1. catalog: read the lock (a corrupt lock fails here).
2. resolve: `resolver.rs`. Git: `check_url`, subdir path check,
   `git ls-remote <url>`.
3. fetch: `source.rs`. Git: `init --bare`, `fetch --depth=1 --no-tags
   <url> <sha>`, `rev-parse --verify <sha>^{commit}`, `ls-tree -r -l -z`
   check, `read-tree <sha>[:subdir]`, `checkout-index --all`. Local: copy,
   symlinks rejected. Bundled: every path checked before it is written.
4. validate: `SkillManifest::read`; name must equal the bundled name, the
   local dir name, or the last subdir component (no check for a git
   repository root).
5. verify: `integrity::tree_digest`: no symlink, no `..`, no absolute
   path, ≤512 files, ≤1 MiB per file, ≤8 MiB total. Digest: sha256 over
   sorted `<path>\0<sha256 hex>\n`.
6. materialize: rename into `skills/<id>/<version>/`.
7. lock.

Credentials: `check_url` rejects userinfo and any query part with
`token=` (so `access_token=` too), in `SkillSource::parse` and again in
resolve, before any git call. The error does not carry the URL. Only
`https://host/…` and `file:///…` are accepted, which also blocks `ext::`,
`ssh` and option-like URLs.

## Tests

`crates/horch-marketplace/tests/marketplace.rs`, 14 tests: `mkt_01`,
`mkt_02`, `mkt_03`, `mkt_04`, `mkt_05`, `mkt_06_rejects_{traversal,
symlink,oversize,invalid_skill_md,hooks}`, `mkt_07`, `mkt_08`, `mkt_10`,
`nfr_07`. Plus 8 unit tests in `src/`. `HORCH_REQUIRE_GIT=1 cargo test -p
horch-marketplace`: 22 of 22 pass.

- `fixture::git` is the only process spawn in the test file. It asserts
  the dir is under `std::env::temp_dir()` and sets `GIT_CONFIG_GLOBAL` to
  an empty temp file and `GIT_CONFIG_NOSYSTEM=1`. `nfr_07` checks this by
  scan.
- `mkt_10` finds pattern positions of `SkillSource::` and
  `ResolvedOrigin::` (followed by `=>`, `|`, `if`, or `=`) and allows
  them only in `resolver.rs` and `source.rs`. `SkillSource::with_subdir`
  and `pinned` live in `resolver.rs` for this reason.

## Decisions and gotchas

- The fixture bare repo needs `init -b main`; with an empty global config
  its HEAD points at `master` and `ls-remote` lists no `HEAD`.
- `src/` must not contain the words `horch_core` or `teammate`, even in
  comments (`mkt_01`).
- `reinstall_from_lock` stops at the first failing entry and does not
  change the lock. A rebuilt dir must match the locked digest.
- No inter-process lock on the store yet. Two concurrent installs can race
  on `marketplace.lock` (last writer wins). Core's `DirLock` (A2+) can
  wrap `Installer` calls.
- Local copy checks file types with `symlink_metadata`, then copies; a
  file swapped for a symlink in between is followed. Local sources are
  the operator's own directories, so this is accepted.
- `fetch --depth=1` still downloads the whole commit tree before the
  listing check; the limits bound what is written, not what is fetched.

## Gate (interim rule)

fmt, `build --all-targets`, `build --bins`, `teammates --check` and
`check-deps.sh` pass. `cargo test --workspace`: 47 failing tests, equal to
the base count, 0 in this crate. `verify-telemetry-e2e.sh` fails in
`horch-e2e/tests/scenario.rs::the_hermetic_story`: the golden
`crates/horch-e2e/tests/golden/telemetry-e2e.txt` is missing. That test is
one of the 47 workspace failures; this unit does not touch `horch-e2e`.

## SPEC-TODO

- `SPEC-TODO(Spec A §10)` in `src/manifest.rs`: the SKILL.md key
  allowlist (`name description license metadata allowed-tools`).
