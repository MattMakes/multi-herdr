cwd := invocation_directory()
horch := "cargo run --quiet --bin horch --"

export HORCH_TEAMMATES_DIR := justfile_directory() / "teammates"

# List available recipes.
default:
    @just --list

# Build the release binaries and install horch, herdr-install and the herdr-fleet and herdr-run launchers into ~/.local/bin.
install:
    cargo build --release --bin horch --bin herdr-install
    ./target/release/horch install
    ./scripts/remove-zsh-fleet-function ~/.zshrc
    @echo "Installed: $(~/.local/bin/horch --version). Open a new shell, then run: herdr-fleet or herdr-run"

# Fail fast with a clear message if herdr is missing or unreachable.
require-herdr:
    {{horch}} doctor

# Launch the fixed 5-pane workspace: 1 orchestrator and 4 workers, wired for two-way messaging.
herdr-orchestration: require-herdr
    {{horch}} orchestration --cwd "{{cwd}}"

# Launch a fleet: 1 orchestrator pane that spawns the workers it needs. FLAVOR picks the orchestrator: opus, fable, astra or sol.
herdr-fleet FLAVOR="opus": require-herdr
    {{horch}} fleet {{FLAVOR}} --cwd "{{cwd}}"

# Check the fleet machinery end to end with a token-free fake agent in a scratch workspace.
herdr-fleet-smoke: require-herdr
    {{horch}} smoke fleet

# Check `horch tile` with token-free shell panes in a scratch workspace.
horch-tile-smoke: require-herdr
    {{horch}} smoke tile

# Check the herdr messaging primitives with 2 panes and no agents. Run this first on a new herdr install.
horch1-smoke: require-herdr
    {{horch}} smoke messaging

# Show the roster the orchestrator picks from.
teammates:
    {{horch}} teammates

# The roster as a tuning table: harness, model, effort, phase, expected skills, price.
teammates-matrix:
    {{horch}} teammates --matrix

# What the fleet run in the calling directory cost, per worker. Extra flags pass through, e.g. `--since 2026-09-24`.
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

# Everything the telemetry spec asks before a milestone is claimed (docs/specs/telemetry.md, section 16.3).
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

# Live check, slow: cargo test --workspace and Godot headless in aarch64 Linux. Starts colima and stops it again.
test-linux:
    ./scripts/live/linux.sh

# NFR-02, slow: the collector's tick and cold-start budgets over a generated 1 GB corpus. Not part of `verify`.
verify-perf:
    cargo test --release -p horch-core --test nfr -- --ignored --nocapture nfr_02
