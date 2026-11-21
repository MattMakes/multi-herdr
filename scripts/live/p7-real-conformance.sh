#!/bin/sh
# The release gate of the real agent CLIs (finding R6-F4 of
# docs/specs/fleetd/reviews/end-review-p7.md; design FR-6, FR-8, SR-1, SR-7,
# SR-8, FM-18, FM-19).
#
# The gate runs the production conformance suite (`horch job run`, started
# by fleetd) 1 time for each installed CLI, in a temp fleet: a temp
# FLEET_ROOT, HORCH_LEDGER_DB, HORCH_STATE_DIR and HORCH_DATA_DIR. It
# copies each <fleet root>/docs/<cli>/<version>/conformance.json to
# <out>/<cli>/<version>/conformance.json, and it passes only when each
# check of each report has the verdict "pass". On exit it stops its fleetd
# and removes the temp fleet.
#
# Operator release step:
#   A worker never runs this gate with the real CLIs. A real run uses the
#   login and the quota of the operator, and a worker never uses
#   ANTHROPIC_API_KEY. Before a release, the operator runs in a logged-in
#   shell:
#     env -u ANTHROPIC_API_KEY scripts/live/p7-real-conformance.sh --out docs/specs/fleetd/gates/evidence/p7-conformance
#   and records each conformance.json path that it prints in
#   docs/specs/fleetd/gates/fleetd-p7.md.
#
# Environment:
#   HORCH_BIN                 the horch binary (default: horch on PATH).
#   P7_CONFORMANCE_TIMEOUT_S  the wait for the runs, in seconds (default 1800).
#   HORCH_TEST_DOCS_FIXTURE   tests only: the docs sync reads a local folder.
set -eu

unset ANTHROPIC_API_KEY GEMINI_API_KEY GOOGLE_API_KEY GOOGLE_GEMINI_BASE_URL

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/docs/specs/fleetd/gates/evidence/p7-conformance"
CLIS="claude codex"

usage() {
    cat <<'EOF'
usage: scripts/live/p7-real-conformance.sh [--out <evidence dir>] [--clis "claude codex"]

Run the production conformance suite 1 time for each CLI in a temp fleet.
Keep <evidence dir>/<cli>/<version>/conformance.json. Exit 0 only when
each check of each report passes.

  --out <dir>    the evidence folder
                 (default: docs/specs/fleetd/gates/evidence/p7-conformance)
  --clis <list>  the CLIs, separated by spaces (default: "claude codex")

Operator release step:
  A worker never runs this gate with the real CLIs. A real run uses the
  login and the quota of the operator, and a worker never uses
  ANTHROPIC_API_KEY. Before a release, the operator runs in a logged-in
  shell:
    env -u ANTHROPIC_API_KEY scripts/live/p7-real-conformance.sh --out docs/specs/fleetd/gates/evidence/p7-conformance
  and records each conformance.json path that it prints in
  docs/specs/fleetd/gates/fleetd-p7.md.
EOF
}

die() {
    echo "p7-real-conformance: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
    --out)
        [ "$#" -ge 2 ] || die "--out needs a folder"
        OUT="$2"
        shift 2
        ;;
    --clis)
        [ "$#" -ge 2 ] || die "--clis needs a list"
        CLIS="$2"
        shift 2
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
    esac
done

HORCH_BIN="${HORCH_BIN:-$(command -v horch || true)}"
[ -x "$HORCH_BIN" ] || die "no horch binary: set HORCH_BIN"
TIMEOUT="${P7_CONFORMANCE_TIMEOUT_S:-1800}"
SQLITE3="$(command -v sqlite3 || true)"
[ -n "$SQLITE3" ] || die "the gate reads the ledger with sqlite3, and sqlite3 is not on PATH"
GIT="$(command -v git || true)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

# A short temp folder: macOS limits a Unix socket path to 104 bytes.
case "$(uname -s)" in
Linux) TMP_BASE=/var/tmp ;;
*) TMP_BASE=/tmp ;;
esac
TMP="$(mktemp -d "$TMP_BASE/p7rc.XXXXXX")"
FLEETD_PID=""

cleanup() {
    if [ -n "$FLEETD_PID" ]; then
        kill -TERM "$FLEETD_PID" 2>/dev/null || true
        wait "$FLEETD_PID" 2>/dev/null || true
    fi
    rm -rf "$TMP"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

BIN="$TMP/bin"
mkdir -p "$BIN"
for cli in $CLIS; do
    path="$(command -v "$cli" || true)"
    [ -n "$path" ] || die "$cli is not on PATH"
    ln -s "$path" "$BIN/$cli"
done
ln -s "$SQLITE3" "$BIN/sqlite3"
if [ -n "$GIT" ]; then
    ln -s "$GIT" "$BIN/git"
fi
DOCS_SOURCE_BINS="$(awk -F'"' '/^version_cmd/ { split($2, w, " "); print w[1] }' \
    "$ROOT/crates/agent-docs-sync/docs-sources.toml")"
FLEET_PATH="$BIN"
old_ifs="$IFS"
IFS=:
for dir in $PATH; do
    keep=1
    for tool in $DOCS_SOURCE_BINS; do
        if [ -x "$dir/$tool" ]; then
            keep=0
        fi
    done
    if [ "$keep" = 1 ] && [ -n "$dir" ]; then
        FLEET_PATH="$FLEET_PATH:$dir"
    fi
done
IFS="$old_ifs"

FLEET_ROOT="$TMP/fleet"
HORCH_LEDGER_DB="$TMP/ledger.sqlite3"
HORCH_STATE_DIR="$TMP/state"
HORCH_DATA_DIR="$TMP/data"
export FLEET_ROOT HORCH_LEDGER_DB HORCH_STATE_DIR HORCH_DATA_DIR
export HORCH_NO_WAKE=1 HORCH_NO_WATCHDOG=1 HORCH_NO_UPDATE=1
mkdir -p "$FLEET_ROOT" "$HORCH_STATE_DIR" "$HORCH_DATA_DIR"
echo "pass_period_s = 1" >"$FLEET_ROOT/fleet.toml"
echo "fleet root: $FLEET_ROOT"

PATH="$FLEET_PATH" "$HORCH_BIN" fleetd </dev/null >"$TMP/fleetd.stdout" 2>"$TMP/fleetd.stderr" &
FLEETD_PID=$!
echo "fleetd pid: $FLEETD_PID"

sql() {
    "$SQLITE3" -readonly -cmd ".timeout 5000" "$HORCH_LEDGER_DB" "$1" 2>/dev/null || true
}

diagnostics() {
    echo "jobs:"
    sql "SELECT id || ' ' || kind || ' ' || subject || ' ' || state FROM jobs ORDER BY id"
    echo "fleetd stderr:"
    tail -n 40 "$TMP/fleetd.stderr"
}

wait_until() {
    what="$1"
    shift
    waited=0
    while ! "$@"; do
        if ! kill -0 "$FLEETD_PID" 2>/dev/null; then
            diagnostics >&2
            die "fleetd exited before $what"
        fi
        if [ "$waited" -ge "$TIMEOUT" ]; then
            diagnostics >&2
            die "no $what after $TIMEOUT s"
        fi
        sleep 1
        waited=$((waited + 1))
    done
}

serving() {
    [ -S "$FLEET_ROOT/run/fleetd.sock" ] && [ -f "$FLEET_ROOT/run/operator.token" ]
}
wait_until "command channel" serving

mode() {
    stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"
}
[ "$(mode "$FLEET_ROOT/run/operator.token")" = 600 ] || die "the token file is not mode 0600"
wait_until "request folder" test -d "$FLEET_ROOT/run/requests"
[ "$(mode "$FLEET_ROOT/run/requests")" = 700 ] || die "the request folder is not mode 0700"

ended() {
    for cli in $CLIS; do
        state="$(sql "SELECT state FROM jobs WHERE kind = 'conformance' AND subject LIKE '$cli %'
            AND state IN ('passed', 'failed', 'abandoned') ORDER BY id LIMIT 1")"
        [ -n "$state" ] || return 1
    done
}
wait_until "ended conformance run of each CLI ($CLIS)" ended

idle() {
    [ "$(sql "SELECT count(*) FROM jobs WHERE state IN ('queued', 'running')")" = 0 ]
}
wait_until "end of each job" idle

kill -TERM "$FLEETD_PID" 2>/dev/null || true
wait "$FLEETD_PID" 2>/dev/null || true
FLEETD_PID=""

status=0
summary=""
for cli in $CLIS; do
    found=0
    for report in "$FLEET_ROOT/docs/$cli"/*/conformance.json; do
        [ -f "$report" ] || continue
        found=1
        version="$(basename "$(dirname "$report")")"
        mkdir -p "$OUT/$cli/$version"
        cp "$report" "$OUT/$cli/$version/conformance.json"
        echo "retained: $OUT/$cli/$version/conformance.json"
        rows="$(awk -F'"' -v cli="$cli" -v version="$version" '
            $2 == "name" { name = $4 }
            $2 == "verdict" { printf "%-8s %-24s %-28s %s\n", cli, version, name, $4 }
        ' "$report")"
        [ -n "$rows" ] || {
            rows="$(printf '%-8s %-24s %-28s %s' "$cli" "$version" "(no checks)" "fail")"
        }
        summary="$summary$rows
"
    done
    if [ "$found" = 0 ]; then
        echo "p7-real-conformance: no conformance.json for $cli" >&2
        status=1
    fi
done

echo
printf '%-8s %-24s %-28s %s\n' CLI VERSION CHECK VERDICT
printf '%s' "$summary"
failed="$(printf '%s' "$summary" | awk '$NF != "pass" { print }')"
if [ -n "$failed" ]; then
    echo
    echo "p7-real-conformance: these checks do not pass:" >&2
    printf '%s\n' "$failed" >&2
    status=1
fi
exit "$status"
