# U06 b1-primitives: fsx, digest, PRNG, redaction, money

Unit slug: `b1-primitives`. Branch: `ard/b1-primitives`. Phase: B1 (part).
Requirements covered: MEA-07, MEA-09, SEC-01 (unit part), SEC-05 (fsx part),
MEA-01 (digest part).

## GOAL

Pure, dependency-free building blocks that phases A6 and B1 to B6 use:
durable atomic file writes and a directory lock (`fsx.rs`), canonical-JSON
sha256 digests, an in-repo SplitMix64 PRNG, secret redaction, and exact
integer money. Each has unit tests. Nothing calls them yet.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Operator decisions" (OD6), "Hard
  constraints", §2 "measure" and "money", §3 "A6" step 1 (DirLock), §3 "B1",
  §4 MEA, SEC and NFR-11 rows.
- Existing locks to generalize (read them, do not change them):
  - `crates/horch-core/src/ledger.rs` lines 143 to 260: `LockGuard`, a
    `create_dir` mkdir lock with stale handling.
  - `crates/horch-core/src/telemetry/lock.rs`: a pid lock (`acquire`,
    `pid_alive`, `LockInfo`).
  Phase A6 switches the ledger and telemetry to `fsx::DirLock`. You only add
  `DirLock`; do not switch any caller.
- Prices today: `crates/horch-core/src/usage.rs`, `struct Price` (line 79),
  `builtin_prices()` (line 117), `load_prices()` (line 137). Prices are
  `f64` dollars per million tokens.
- U01 adds `sha2` to the allowlists (`scripts/check-deps.sh`, `nfr_05`).
  Your branch adds `sha2 = "0.10"` to `crates/horch-core/Cargo.toml`. Until
  U01 merges, `check-deps.sh` and `nfr_05` fail on your branch; that is
  expected. Before READY-TO-MERGE, rebase after U01 merged so the gate is green.
- U05 (A1) adds `ids.rs`, `harness/`, `execution/` at the same time. You own
  the `Digest` type; U05 does not define one.

## FILES

own:
- `crates/horch-core/src/fsx.rs` (new)
- `crates/horch-core/src/measure/mod.rs` (new; declares only your modules)
- `crates/horch-core/src/measure/digest.rs` (new)
- `crates/horch-core/src/measure/testkit.rs` (new)
- `crates/horch-core/src/measure/redact.rs` (new)
- `crates/horch-core/src/usage/money.rs` (new; declared from `usage.rs` with one line `pub mod money;`)
- `crates/horch-core/src/usage.rs` (that one line only)
- `crates/horch-core/src/lib.rs` (add `pub mod fsx; pub mod measure;`)
- `crates/horch-core/Cargo.toml` (add `sha2`)
- `Cargo.lock`
- `ai_docs/reports/arch-refactor-dataset/b1-primitives.md`

do not touch: `ledger.rs`, `telemetry/**`, every other source file, scripts.

## STEPS

1. Create the worktree (conventions §2).
2. `fsx.rs`:
   - `write_atomic(path, bytes, mode: u32)`: write a temp file in the same
     dir (name `.<file>.tmp-<pid>-<random>`), `sync_all`, set the mode on
     unix, `rename`, then fsync the parent dir (unix; no-op on windows).
   - `replace_durable(path, bytes, mode)`: the same as `write_atomic`; it is
     the name the dataset code uses for "replace".
   - `create_immutable(path, bytes, mode) -> Result<Created>` where
     `enum Created { Written, AlreadyIdentical }`: open with `create_new`;
     if the file exists with identical bytes, return `AlreadyIdentical`; if it
     exists with different bytes, return an error `FsxError::Conflict`. Write,
     `sync_all`, fsync the parent dir.
   - `ensure_private_dir(path)`: create with mode 0700 (unix).
     Files default to 0600 where the caller passes `PRIVATE_FILE = 0o600`.
   - `DirLock::acquire(dir, name, stale_after: Duration) -> Result<DirLockGuard>`:
     a `create_dir` lock at `<dir>/<name>.lock/` with an `owner` file holding
     `{pid, host, acquired_at}`. Retry with a short backoff up to a timeout
     parameter. Break the lock when the owner pid is dead (reuse the logic of
     `telemetry::lock::pid_alive` by copying it into a private helper, or call
     it) or when the lock is older than `stale_after`. Release on drop.
   - A hand-written `FsxError` enum with `Display` and `std::error::Error`.
   - Tests: `mea_09_create_immutable_refuses_overwrite`,
     `mea_09_replace_durable`, `sec_05_permissions` (dir 0700, files 0600,
     `cfg(unix)`), `dirlock_excludes_second_holder`, `dirlock_breaks_dead_owner`.
3. `measure/digest.rs`:
   - `pub struct Digest(pub [u8; 32])` with `Display` as `sha256:<64 hex>`,
     `FromStr`, serde as that string, `short12()` (first 12 hex chars).
   - `sha256_bytes(&[u8]) -> Digest`.
   - `canonical_json(&serde_json::Value) -> String`: object keys sorted
     (recursively), no insignificant whitespace, numbers as serde_json prints
     them, strings with serde_json escaping.
   - `digest_json<T: Serialize>(&T) -> Digest` = sha256 of `canonical_json`.
   - Tests: `mea_01_digests_stable` (fixed inputs → fixed hex values written
     into the test; key order and whitespace do not change the digest).
4. `measure/testkit.rs`: `pub struct SplitMix64(u64)` with `new(seed)`,
   `next_u64()`, `next_f64()` in [0, 1), `below(n)` without modulo bias,
   `shuffle<T>(&mut [T])` (Fisher-Yates), `seed_from_digest(&Digest) -> u64`
   (first 8 bytes big-endian). Plus `pub fn property<F: FnMut(&mut SplitMix64)>(seed: u64, cases: u32, f: F)`
   that runs `f` with a fresh generator per case and reports the failing case
   index and seed on panic. Tests: the first 5 outputs for seed 0 and for
   seed `0x9E3779B97F4A7C15` match the published SplitMix64 reference
   (compute them from the reference algorithm and write them as constants),
   `below` stays in range over 10 000 draws, `shuffle` is deterministic per seed.
   This module is `pub` (not `cfg(test)`), because the planner (B3) uses the
   same PRNG for the exploration slot.
5. `measure/redact.rs`: `redact(&str) -> Cow<str>` and
   `redact_env_value(key, value)`. Replace each match with `[REDACTED]`.
   Patterns (hand-written matchers, no regex crate):
   `ANTHROPIC_API_KEY=<value>`, `sk-ant-…`, `sk-…` (at least 20 chars of
   `[A-Za-z0-9_-]` after the prefix), `ghp_…`, `github_pat_…`, `xox?-…`
   (Slack: `xoxb-`, `xoxp-`, `xoxa-`, `xoxr-`, `xoxs-`), `AKIA` plus 16
   uppercase alphanumerics, PEM blocks from `-----BEGIN ... PRIVATE KEY-----`
   to the matching `-----END ... -----`, and
   `(api_key|secret|token|password)` followed by optional spaces and `:` or
   `=` then the value up to whitespace or a quote (case-insensitive key).
   Tests: `sec_01_redaction_patterns` (one positive and one negative case per
   pattern; ordinary text such as `task-123` and `sketch` is untouched).
6. `usage/money.rs`:
   - `pub struct MicroUsd(pub i64)`, `pub struct NanoUsd(pub i128)`.
   - `nano_per_token(dollars_per_mtok: f64) -> Result<i128>`: multiply by
     1000, require the result to be an integer within 1e-9, else return an
     error (`InexactPrice`). Test that every price in `builtin_prices()` is exact.
   - Accumulate in `NanoUsd` (token count × nano per token). Convert to
     `MicroUsd` once with round-half-even (`NanoUsd::to_micro_half_even()`).
   - `enum CostSource { PriceTable { date: String }, PricingFile { sha12: String }, HarnessReported, Unpriced }`
     with `Display` exactly `price_table@<date>`, `pricing_file@<sha12>`,
     `harness_reported`, `unpriced`, and serde as that string.
   - Tests: `mea_07_nano_accumulation_exact`, `mea_07_rounding_once`
     (half-even at .5 µ$: 1500 n$ → 2 µ$, 2500 n$ → 2 µ$, 3500 n$ → 4 µ$;
     summing then rounding differs from rounding each term in a test case),
     `mea_07_cost_source`.
7. Run the gate after each step. Commit per step: `B1: Add fsx durable writes and DirLock`,
   `B1: Add canonical JSON digests`, `B1: Add SplitMix64 testkit`,
   `B1: Add secret redaction`, `B1: Add integer money`.
8. Write and commit the report (public API of each module). Rebase after U01
   merged, gate green, then follow conventions §6.

## CONSTRAINTS

- No regex crate, no new crate except `sha2`.
- No `std::env` reads.
- Every pub item has a one-line doc comment.

## DONE WHEN

- The named tests pass.
- `scripts/check-deps.sh` passes after the rebase on U01.
- The full gate is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the public API per module and gotchas.
