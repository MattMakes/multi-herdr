#!/bin/bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN="$HERE/godot-run.sh"
PATTERN='SCRIPT ERROR|Parse Error|Failed to load script|Nothing was run'

die() { printf 'godot-check: %s\n' "$1" >&2; exit 2; }

[ -f ./project.godot ] || die "no ./project.godot; run from the project root"

gut=0
for arg in "$@"; do
  case "$arg" in
    --gut) gut=1 ;;
    *) die "unknown argument: $arg" ;;
  esac
done
if [ -f .gutconfig.json ] || [ -f addons/gut/gut_cmdln.gd ]; then gut=1; fi

is_csharp() {
  grep -q '^config/features=.*"C#"' project.godot && return 0
  compgen -G '*.csproj' > /dev/null
}

LOGDIR="$PWD/.godot/horch-home/logs"
mkdir -p "$LOGDIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)-$$"
CHECKLOG="$LOGDIR/godot-check-$STAMP.log"
: > "$CHECKLOG"
ESC="$(printf '\033')"
fail=0

step() {
  local name=$1 code
  shift
  set +e
  "$@" > "$LOGDIR/godot-check-$STAMP-$name.raw" 2>&1
  code=$?
  set -e
  sed -E "s/${ESC}\\[[0-9;?]*[A-Za-z]//g" "$LOGDIR/godot-check-$STAMP-$name.raw" | tr -d '\r' >> "$CHECKLOG"
  rm -f "$LOGDIR/godot-check-$STAMP-$name.raw"
  if [ "$code" -ne 0 ] && [ "$fail" -eq 0 ]; then fail=$code; fi
  if [ "$code" -eq 2 ]; then finish; fi
}

finish() {
  cat "$CHECKLOG"
  if grep -Eq "$PATTERN" "$CHECKLOG"; then
    grep -E "$PATTERN" "$CHECKLOG" >&2 || true
    printf 'godot-check: FAIL: script error or no test ran; log: %s\n' "$CHECKLOG" >&2
    exit 3
  fi
  if [ "$fail" -ne 0 ]; then
    printf 'godot-check: FAIL: exit %s; log: %s\n' "$fail" "$CHECKLOG" >&2
    exit "$fail"
  fi
  printf 'godot-check: ok; log: %s\n' "$CHECKLOG"
  exit 0
}

step import bash "$RUN" --path "$PWD" --import

if is_csharp; then
  step dotnet dotnet build --nologo
fi

n=0
while IFS= read -r script; do
  n=$((n + 1))
  step "parse$n" bash "$RUN" --path "$PWD" --check-only --script "res://${script#./}"
done < <(find . \( -path ./.godot -o -path ./addons \) -prune -o -type f -name '*.gd' -print | LC_ALL=C sort)

if [ "$gut" -eq 1 ]; then
  step gut bash "$RUN" --path "$PWD" -s res://addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json
fi

finish
