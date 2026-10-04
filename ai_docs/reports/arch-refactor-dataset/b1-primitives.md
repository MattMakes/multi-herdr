# U06 b1-primitives: report

Branch `ard/b1-primitives`. Phase B1 (part). Nothing calls these modules yet.

## Public API

### `horch_core::fsx`
- `PRIVATE_FILE = 0o600`, `PRIVATE_DIR = 0o700`.
- `write_atomic(path, bytes, mode) -> Result<()>`: temp file `.<name>.tmp-<pid>-<12 hex>` in the same dir, created with `mode`, `sync_all`, `rename`, parent dir fsync (no-op off unix).
- `replace_durable(path, bytes, mode)`: the same as `write_atomic`.
- `create_immutable(path, bytes, mode) -> Result<Created>`; `enum Created { Written, AlreadyIdentical }`. Different bytes give `FsxError::Conflict`. A failed write removes the half-written file.
- `ensure_private_dir(path)`: `create_dir_all`, then mode 0700 on `path` only (not on parents it created).
- `DirLock::acquire(dir, name, stale_after, timeout) -> Result<DirLockGuard>`: `create_dir` lock at `<dir>/<name>.lock/`, owner file `{pid, host, acquired_at, nonce}` (0600). Backoff 5 ms doubling to 100 ms. Breaks the lock when the owner pid is dead on this host, when the lock dir is older than `stale_after`, or when it has no owner file for 5 s. `DirLockGuard::{path, release}`; release on drop.
- `LockOwner`, `FsxError { Io, Conflict, BadPath, LockTimeout }` (hand-written `Display` + `Error`), `fsx::Result<T>`.

### `horch_core::measure::digest`
- `Digest(pub [u8; 32])`: `Display`/`FromStr`/serde as `sha256:<64 lowercase hex>`, `short12()`, `hex()`. `ParseDigestError`.
- `sha256_bytes(&[u8]) -> Digest`, `canonical_json(&Value) -> String`, `digest_json<T: Serialize + ?Sized>(&T) -> Digest`.

### `horch_core::measure::testkit`
- `SplitMix64(pub u64)`: `new`, `next_u64`, `next_f64` in [0, 1) (top 53 bits), `below(n)` (rejection, no modulo bias, panics on 0), `shuffle` (Fisher-Yates).
- `seed_from_digest(&Digest) -> u64` (first 8 bytes, big-endian).
- `property(seed, cases, f)`: case seeds are drawn from `SplitMix64::new(seed)`. A panic prints the case index, the run seed and the case seed, then propagates.

### `horch_core::measure::redact`
- `REDACTED = "[REDACTED]"`, `redact(&str) -> Cow<str>` (borrows when clean), `redact_env_value(key, value) -> Cow<str>`.
- Token shapes are replaced whole: `sk-ant-` (8+), `sk-` (20+), `ghp_` (20+), `github_pat_` (20+), `xox[bpars]-` (10+), `AKIA`+16, PEM blocks whose label contains `PRIVATE KEY` (an unterminated block is redacted to the end).
- `KEY=value` shapes keep the key and separator: `ANTHROPIC_API_KEY`, `api_key`, `secret`, `token`, `password` (case-insensitive, substring match, so `GITHUB_TOKEN=` and `access_token:` match). Key and value may be quoted; a quoted value runs to its closing quote and honours backslash escapes.
- `redact_env_value` redacts the whole value when the key contains KEY, SECRET, TOKEN, PASSWORD, PASSWD or CREDENTIAL.

### `horch_core::usage::money`
- `MicroUsd(pub i64)` (serde transparent), `NanoUsd(pub i128)` with `of`, `add_tokens`, `Add`, `AddAssign`, `Sum`, `to_micro_half_even()` (saturates at the i64 range).
- `nano_per_token(f64) -> Result<i128, MoneyError>`; `MoneyError { InexactPrice { dollars_per_mtok }, Overflow }` as in the dataset design §4.4.
- `CostSource { PriceTable { date }, PricingFile { sha12 }, HarnessReported, Unpriced }`: `Display`/`FromStr`/serde as `price_table@<date>`, `pricing_file@<sha12>` (12 lowercase hex), `harness_reported`, `unpriced`.

## Tests added (21)

mea_09_create_immutable_refuses_overwrite, mea_09_replace_durable, sec_05_permissions, dirlock_excludes_second_holder, dirlock_breaks_dead_owner, dirlock_breaks_old_lock, dirlock_stale_holder_does_not_release_the_next_holder, mea_01_digests_stable, digest_string_round_trips, nfr_11_splitmix64_matches_reference, below_stays_in_range, shuffle_is_deterministic_per_seed, seed_from_digest_is_first_8_bytes_big_endian, property_runs_every_case_and_reports_failures, sec_01_redaction_patterns, redaction_keeps_surrounding_unicode, env_values_redact_by_key, mea_07_every_builtin_price_is_exact, mea_07_nano_accumulation_exact, mea_07_rounding_once, mea_07_cost_source.

## Decisions

- `DirLock::acquire` takes a 4th parameter, `timeout`, because the plan asks for "a timeout parameter".
- The owner file has a 4th field, `nonce`. A guard removes the lock only while the nonce is still its own. Without it, a holder that outlived `stale_after` deletes the next holder's lock on drop.
- The host name comes from `libc::gethostname`, not from `HOSTNAME`. Reason: no `std::env` reads in core. `acquired_at` uses `chrono::Utc::now()` directly, because `clock::now` reads `HORCH_NOW`.
- `pid_alive` is copied privately from `telemetry::lock`. Phase A6 must fold the two copies into one.
- `canonical_json` sorts keys itself and does not rely on the `BTreeMap` order, because the serde_json `preserve_order` feature can be switched on by another crate.
- Negative and NaN prices are `InexactPrice`, because the design's `MoneyError` has no third variant.

## Security self-review (redact.rs, fsx.rs)

Fixed on this branch: a stale DirLock holder releasing the next holder's lock; files that briefly existed with umask permissions; quoted values with escaped quotes leaking their tail; PGP private key blocks missed.

Known limits (not fixed, outside the listed patterns):
- `Authorization: Bearer <token>` and other bearer headers are not redacted.
- The keyword match has no left word boundary, so `nosecret=x` is redacted (over-redaction, safe).
- `ensure_private_dir` sets 0700 on the leaf only. Callers must call it for each directory under the dataset root, as the dataset design §3 table says.
- `create_immutable` follows a symlink when it compares existing content. The dataset root is 0700, so only the same user can plant one.

## For later phases

- A6: switch `ledger.rs` `LockGuard` and `telemetry/lock.rs` to `fsx::DirLock`, and remove the private `pid_alive` copy.
- B1 store: use `DirLock::acquire(root, "events", ..)` for `events.lock/`.
- B3: seed the exploration slot with `seed_from_digest(&sha256_bytes(round_id.as_bytes()))`.
