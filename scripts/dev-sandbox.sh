#!/bin/sh
# Run <command...> in an isolated horch sandbox (ai_docs/plans/fleetd-finish/audit/isolation.md,
# section "Sandbox env"). A development build then never reads or writes the live
# ledger, state, data, Claude or Codex homes of the operator.
#
# The run folder <worktree>/.dev-sandbox/run.<pid> holds home, state, data,
# config and cache. tmp and ledger live in 1 short folder under /tmp, and the
# trap removes both folders on exit and on INT or TERM.
# Reason: macOS limits a Unix socket path (sun_path) to 104 bytes, and a socket under the worktree is longer.
set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: scripts/dev-sandbox.sh <command...>" >&2
    exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN="$ROOT/.dev-sandbox/run.$$"
SHORT=""

cleanup() {
    rm -rf "$RUN"
    rmdir "$ROOT/.dev-sandbox" 2>/dev/null || true
    if [ -n "$SHORT" ]; then
        rm -rf "$SHORT"
    fi
}
trap cleanup EXIT
trap 'cleanup; trap - EXIT; exit 130' INT
trap 'cleanup; trap - EXIT; exit 143' TERM

mkdir -p "$RUN/home" "$RUN/state" "$RUN/data" "$RUN/config" "$RUN/cache"
SHORT="$(mktemp -d /tmp/hds.XXXXXX)"
mkdir -p "$SHORT/tmp" "$SHORT/ledger"
ln -s "$SHORT/tmp" "$RUN/tmp"
ln -s "$SHORT/ledger" "$RUN/ledger"

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME RUSTUP_HOME

unset FLEET_ROOT HORCH_ALLOW_LEDGER_MIGRATE ANTHROPIC_API_KEY

export HOME="$RUN/home"
export TMPDIR="$SHORT/tmp"
export HORCH_STATE_DIR="$RUN/state"
export HORCH_DATA_DIR="$RUN/data"
export HORCH_LEDGER_DB="$SHORT/ledger/ledger.sqlite3"
export XDG_CONFIG_HOME="$RUN/config"
export XDG_CACHE_HOME="$RUN/cache"
export CODEX_HOME="$RUN/home/.codex"
export CLAUDE_CONFIG_DIR="$RUN/home/.claude"

# Not exec: the trap must run after the command ends.
status=0
"$@" || status=$?
exit "$status"
