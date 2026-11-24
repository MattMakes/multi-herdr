# Live check: Cargo in a Codex sandbox

This check proves whether `cargo check` and `cargo clippy` work inside a
macOS Codex `workspace-write` sandbox, and that the `CARGO_HOME` fallback
of `crates/horch-core/src/harness/codex.rs` fixes the failing case. It is
GRS-17 of `docs/specs/godot-rust-support.md` (G12).

The sandbox lets a pane write only its workdir, the horch state root and
its temp folders. `~/.cargo` is outside them, so it is read-only.

## What horch does

When a Codex pane has sandboxed writes (not `read-only`, not
`danger-full-access`) and network access, horch sets
`shell_environment_policy.set.CARGO_HOME` to `<state_root>/cargo` and
creates that folder. All Codex panes share it, and it persists after a pane
closes. horch does not set it when:

- the teammate `shell_env` sets `CARGO_HOME`.
- `~/.cargo` is under the workdir or the state root.
- the pane has no network. An empty `CARGO_HOME` cannot download, and a
  read-only `~/.cargo` still builds dependencies that it already holds
  (step `fallback-no-network` below).

Warning: the fallback does not read `~/.cargo/config.toml`. Put a needed
Cargo setting in the project `.cargo/config.toml`, or set `CARGO_HOME` in
the teammate `shell_env`.

## How to run

No script exists. The steps use `codex sandbox` with the `fleet_write`
profile that `codex-sol` panes use (`fleet_write_profile_args` in
`codex.rs`). `codex sandbox` applies the same seatbelt policy as a pane and
starts no model session.

1. Make 2 standalone crates (each with an empty `[workspace]` table) under
   `.worktrees/_scratch/live-codex-cargo-home/`:
   - `fetched` depends on `itoa = "=1.0.18"`, which `~/.cargo/registry`
     already holds. Run `cargo generate-lockfile --offline` in it.
   - `unfetched` depends on `bitflags = "=0.1.1"`, which it does not hold.
2. Run each step from the crate folder:

```bash
ST=~/.local/state/horch; P=fleet_write
codex sandbox \
  -c "permissions.$P.extends=\":workspace\"" \
  -c "permissions.$P.filesystem={\":project_roots\"={\".\"=\"write\",\".git\"=\"write\"},\":slash_tmp\"=\"read\",\":tmpdir\"=\"read\"}" \
  -c "permissions.$P.workspace_roots={\"$ST\"=true}" \
  -c "permissions.$P.network.enabled=true" \
  -P $P -C "$PWD" -- /usr/bin/env CARGO_HOME="$ST/tmp/live-cargo-home" cargo check
```

Leave out `CARGO_HOME=...` for the default steps. Set
`network.enabled=false` for the no-network steps. Remove
`$ST/tmp/live-cargo-home` and each `target/` between steps.

## 2026-11-24

Host: macOS 26.5.1 (arm64), Codex CLI 0.166.0, cargo 1.99.1 (Homebrew).

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| default-write-probe | `~/.cargo` is read-only in the sandbox | codex-cli 0.166.0 | PASS | `touch ~/.cargo/.probe`: `Operation not permitted` |
| default-fetched | a held dependency builds with the read-only `~/.cargo` | cargo 1.99.1 | PASS | `cargo check` exit 0, `Checking itoa v1.0.18` |
| default-unfetched | a new dependency fails with the read-only `~/.cargo` | cargo 1.99.1 | FAIL (expected) | exit 101: `failed to open .../registry/cache/.../bitflags-0.1.1.crate`, `Operation not permitted (os error 1)` |
| fallback-fetched | the state-root `CARGO_HOME` builds a held dependency, in `codex.rs` `cargo_home_args` | cargo 1.99.1 | PASS | exit 0, `Downloaded itoa v1.0.18` into the fallback |
| fallback-unfetched | the state-root `CARGO_HOME` builds a new dependency | cargo 1.99.1 | PASS | exit 0, `Downloaded bitflags v0.1.1`, `Checking unfetched v0.1.0` |
| fallback-clippy | `cargo clippy` works with the fallback | cargo 1.99.1 | PASS | exit 0, `Finished dev profile` |
| fallback-no-network | the fallback needs network, so horch skips it without network | cargo 1.99.1 | FAIL (expected) | exit 101: `Could not resolve host: index.crates.io` |
| default-no-network | a held dependency builds offline with `~/.cargo` | cargo 1.99.1 | PASS | exit 0, `Checking fetched v0.1.0` |

No step started a model session.
