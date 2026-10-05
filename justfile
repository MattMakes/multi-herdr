# multi-herdr: "horch" multi-agent orchestration layouts for herdr
# (https://herdr.dev).
#
# horch (crates/horch) is the actual implementation now - a single Rust
# binary, no bash/jq/node required. For day-to-day use run `just install`
# once: it puts the release build on your PATH as `horch`, plus a
# `herdr-fleet` launcher you can run from any project. Run it again after
# any change (yours or a `git pull`) to reinstall. The other recipes are a thin, optional `just` front end
# over horch for people who like typing `just herdr-fleet`; they build horch
# on demand via `cargo run`, so they always run the current source tree,
# never a stale installed copy - useful for development.
#
# All recipes need a running herdr server (launch the herdr app, or
# `herdr server` headless), and work from ANY terminal - herdr's socket has
# no client ancestry check.

cwd := invocation_directory()
horch := "cargo run --quiet --bin horch --"

# The roster of team members. Every briefing an agent reads comes from a file
# in here, so it is exported to every recipe: `horch` falls back to a
# compiled-in copy, and without this a fleet launched from another directory
# would silently ignore edits made in this folder.
export HORCH_TEAMMATES_DIR := justfile_directory() / "teammates"

# List available recipes.
default:
    @just --list

# Build the release binaries, install them as ~/.local/bin/horch and
# ~/.local/bin/multi-herdr-dataset, install the herdr-fleet launcher next
# to them, and remove the old herdr-fleet shell function from ~/.zshrc (a
# backup is written first).
install:
    cargo build --release --bin horch --bin multi-herdr-dataset
    ./target/release/horch install
    sed "s|__TEAMMATES_DIR__|{{justfile_directory()}}/teammates|" scripts/herdr-fleet > ~/.local/bin/herdr-fleet
    chmod 755 ~/.local/bin/herdr-fleet
    ./scripts/remove-zsh-fleet-function ~/.zshrc
    @echo "Installed: $(~/.local/bin/horch --version). Open a new shell, then run: herdr-fleet"

# Fail fast with a clear message if herdr is missing or unreachable.
require-herdr:
    {{horch}} doctor

# Launch herdr-orchestration: 1 orchestrator (Claude, Fable) + 4 workers
# (2x Claude Sonnet xhigh, 1x Claude Opus xhigh, 1x Codex CLI), laid out
# as orchestrator-left / 2x2-worker-grid-right, wired for two-way messaging
# via `horch tell`.
herdr-orchestration: require-herdr
    {{horch}} orchestration --cwd "{{cwd}}"

# Launch herdr-fleet: a dynamic multi-agent fleet with a persistent
# per-project session ledger. Starts ONE orchestrator pane and nothing
# else - it breaks the work down and spawns exactly the workers it needs
# with `horch spawn` (new panes, new or RESUMED sessions chosen from the
# ledger by task complexity/continuity); workers record progress with
# `horch note` and shut their own pane down with `horch done` when truly
# finished. Run `just teammates` to see who it can pick from.
#
# FLAVOR picks who orchestrates, and nothing else - the roster of workers
# is the same either way:
#   herdr-fleet        Claude Code on Opus (the default; also cc, claude)
#   herdr-fleet fable  Claude Code on Fable
#   herdr-fleet astra  Codex on Astra (also codex)
#   herdr-fleet sol    Codex on Sol
herdr-fleet FLAVOR="opus": require-herdr
    {{horch}} fleet {{FLAVOR}} --cwd "{{cwd}}"

# Self-verifying check of the fleet machinery (spawn -> brief -> register
# -> ledger add/note/done -> tell -> pane self-close) using a token-free
# fake agent in a scratch workspace with an isolated ledger. Closes and
# cleans everything up on success, leaves the workspace open on failure.
herdr-fleet-smoke: require-herdr
    {{horch}} smoke fleet

# Self-verifying check of `horch tile`: a deliberately bad shape becomes the
# canonical grid across two tabs with every pane and its process alive, a
# spawn lays the grid out by itself, and a worker leaving frees its slot.
# Token-free: the panes are shells counting into a file.
horch-tile-smoke: require-herdr
    {{horch}} smoke tile

# Cheap, self-verifying 2-pane check of the herdr messaging primitives
# (send-text + send-keys enter, and horch's pane registry) without
# booting any agents. Run this first if you haven't verified horch
# against your herdr install yet. Closes its scratch workspace on success.
horch1-smoke: require-herdr
    {{horch}} smoke messaging

# Show the roster the orchestrator picks from.
teammates:
    {{horch}} teammates

# The roster as a tuning table: harness, model, effort, phase, expected
# skills, price. Pair with `just cost` when retuning (see the tune-fleet skill).
teammates-matrix:
    {{horch}} teammates --matrix

# What the fleet run in the directory you called this from cost, per worker,
# from each harness's transcripts. Extra flags pass through, e.g.
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

# Everything the telemetry spec asks before a milestone is claimed
# (docs/specs/telemetry.md, section 16.3).
verify:
    cargo build --workspace --bins
    cargo test --workspace
    ./scripts/check-req-coverage.sh
    ./scripts/verify-telemetry-e2e.sh
    ./scripts/check-deps.sh
    rustfmt --edition 2021 --check $(git diff --name-only --diff-filter=AM main -- '*.rs')
    HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check

# The per-commit gate for the arch-refactor-dataset branch
gate:
    ./scripts/phase-gate.sh

# NFR-02: the collector's tick and cold-start budgets over a generated 1 GB
# corpus. Slow; not part of `verify`.
verify-perf:
    cargo test --release -p horch-core --test nfr -- --ignored --nocapture nfr_02
