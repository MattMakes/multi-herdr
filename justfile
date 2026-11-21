cwd := invocation_directory()
horch := "cargo run --quiet --bin horch --"

export HORCH_TEAMMATES_DIR := justfile_directory() / "teammates"

# List available recipes.
default:
    @just --list

# backup is written first).
install:
    cargo build --release --bin horch --bin herdr-install
    ./target/release/horch install
    ./scripts/remove-zsh-fleet-function ~/.zshrc
    @echo "Installed: $(~/.local/bin/horch --version). Open a new shell, then run: herdr-fleet or herdr-run"

# Fail fast with a clear message if herdr is missing or unreachable.
require-herdr:
    {{horch}} doctor

# via `horch tell`.
herdr-orchestration: require-herdr
    {{horch}} orchestration --cwd "{{cwd}}"

#   herdr-fleet sol    Codex on Sol
herdr-fleet FLAVOR="opus": require-herdr
    {{horch}} fleet {{FLAVOR}} --cwd "{{cwd}}"

# cleans everything up on success, leaves the workspace open on failure.
herdr-fleet-smoke: require-herdr
    {{horch}} smoke fleet

# Token-free: the panes are shells counting into a file.
horch-tile-smoke: require-herdr
    {{horch}} smoke tile

# against your herdr install yet. Closes its scratch workspace on success.
horch1-smoke: require-herdr
    {{horch}} smoke messaging

# Show the roster the orchestrator picks from.
teammates:
    {{horch}} teammates

# skills, price. Pair with `just cost` when retuning (see the tune-fleet skill).
teammates-matrix:
    {{horch}} teammates --matrix

# `just cost --since 2026-09-24 --reprice sonnet`.
cost *ARGS:
    HORCH_PROJECT_DIR="{{cwd}}" {{horch}} cost {{ARGS}}

# Validate every file in teammates/ before a fleet reads them for real.
teammates-check:
    {{horch}} teammates --check

# Scaffold teammates/<NAME>.md from the annotated template.
teammate-new NAME:
    {{horch}} teammates --new "{{NAME}}"

# The skill catalog: `just skills`, `just skills --phase plan --json`, `just skills show tdd`.
skills *ARGS:
    {{horch}} skills {{ARGS}}

# Install and pin a skill: owner/repo[@rev], a git URL[@rev], a directory, or bundled:<id>.
skills-install SOURCE:
    {{horch}} skills install "{{SOURCE}}"

# Re-resolve installed skills (all, or the one named) and install what changed.
skills-update *ID:
    {{horch}} skills update {{ID}}

# Verify every installed skill against its locked digest; fails on a problem.
skills-doctor:
    {{horch}} skills doctor

# Rebuild any locked skill whose files are missing, at its pinned commit.
marketplace-refresh:
    {{horch}} marketplace refresh

# (docs/specs/telemetry.md, section 16.3).
verify:
    cargo build --workspace --bins
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
    cargo test --workspace
    ./scripts/check-req-coverage.sh
    ./scripts/verify-telemetry-e2e.sh
    ./scripts/check-deps.sh
    rustfmt --edition 2021 --check $(git diff --name-only --diff-filter=AM main -- '*.rs')
    HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check

# The per-commit gate
gate:
    ./scripts/phase-gate.sh

# Live check: cargo test --workspace and Godot headless in aarch64 Linux.
test-linux:
    ./scripts/live/linux.sh

# corpus. Slow; not part of `verify`.
verify-perf:
    cargo test --release -p horch-core --test nfr -- --ignored --nocapture nfr_02
