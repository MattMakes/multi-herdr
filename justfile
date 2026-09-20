# multi-herdr: "horch" multi-agent orchestration layouts for herdr
# (https://herdr.dev).
#
# horch (crates/horch) is the actual implementation now - a single Rust
# binary, no bash/jq/node required. For day-to-day use run `just install`
# once: it puts the release build on your PATH as `horch`, plus a
# `herdr-fleet` launcher you can run from any project. `just update` pulls
# and reinstalls. The other recipes are a thin, optional `just` front end
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

# Build the release binary, install it as ~/.local/bin/horch, install the
# herdr-fleet launcher next to it, and remove the old herdr-fleet shell
# function from ~/.zshrc (a backup is written first).
install:
    cargo build --release --bin horch
    ./target/release/horch install
    sed "s|__TEAMMATES_DIR__|{{justfile_directory()}}/teammates|" scripts/herdr-fleet > ~/.local/bin/herdr-fleet
    chmod 755 ~/.local/bin/herdr-fleet
    ./scripts/remove-zsh-fleet-function ~/.zshrc
    @echo "Installed: $(~/.local/bin/horch --version). Open a new shell, then run: herdr-fleet"

# Pull main and reinstall.
update:
    git pull --ff-only
    just install

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
#   herdr-fleet        Claude Code on Fable (the default)
#   herdr-fleet cc     the same, said out loud
#   herdr-fleet codex  Codex on Astra
herdr-fleet FLAVOR="cc": require-herdr
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

# Validate every file in teammates/ before a fleet reads them for real.
teammates-check:
    {{horch}} teammates --check

# Scaffold teammates/<NAME>.md from the annotated template.
teammate-new NAME:
    {{horch}} teammates --new "{{NAME}}"
