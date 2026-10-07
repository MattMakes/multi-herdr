#!/bin/bash
# Run 1 Godot command for a fleet worker. Usage, from the project root:
#   godot-run.sh <godot arguments...>
# Sets private user dirs under .godot/horch-home, adds --headless, logs the
# output, and exits 3 when the log has a sandbox line. In a Codex pane on
# macOS it also writes the 2 TLS overrides of docs/live-checks/godot-codex.md.
# Exit codes: the engine's code, 2 for a setup error, 3 for a sandbox line.
set -euo pipefail

CERT=/etc/ssl/cert.pem

die() { printf 'godot-run: %s\n' "$1" >&2; exit 2; }

MONO_APP=/Applications/Godot_mono.app/Contents/MacOS/Godot

# A C# project: "C#" in config/features of project.godot, or a *.csproj in
# the project root.
is_csharp() {
  grep -q '^config/features=.*"C#"' project.godot && return 0
  compgen -G '*.csproj' > /dev/null
}

# Print the engine path. A C# project gets only the .NET engine: the
# standard engine cannot build or run C#. Order: GODOT_MONO_PATH, the macOS
# Godot_mono app. Any other project: GODOT_PATH, godot on PATH, the macOS app.
find_engine() {
  if is_csharp; then
    local mono="${GODOT_MONO_PATH:-}"
    if [ -z "$mono" ] && [ -x "$MONO_APP" ]; then mono="$MONO_APP"; fi
    [ -n "$mono" ] && [ -x "$mono" ] || return 1
    printf '%s\n' "$mono"
    return 0
  fi
  local engine="${GODOT_PATH:-}"
  if [ -z "$engine" ]; then engine="$(command -v godot || true)"; fi
  if [ -z "$engine" ] && [ -x /Applications/Godot.app/Contents/MacOS/Godot ]; then
    engine=/Applications/Godot.app/Contents/MacOS/Godot
  fi
  [ -n "$engine" ] && [ -x "$engine" ] || return 1
  printf '%s\n' "$engine"
}

# Set network/tls/editor_tls_certificates in the editor settings file, and
# keep every other line. Godot creates the file on the first import.
write_editor_setting() {
  local file=$1
  mkdir -p "$(dirname "$file")"
  if [ ! -f "$file" ]; then
    printf '[gd_resource type="EditorSettings" format=3]\n\n[resource]\n' > "$file"
  fi
  if grep -q '^network/tls/editor_tls_certificates = ' "$file"; then
    CERT="$CERT" perl -pi -e 's#^network/tls/editor_tls_certificates = .*$#network/tls/editor_tls_certificates = "$ENV{CERT}"#' "$file"
  else
    printf 'network/tls/editor_tls_certificates = "%s"\n' "$CERT" >> "$file"
  fi
}

write_override() {
  if [ ! -f override.cfg ]; then
    {
      printf '; horch godot-run.sh: Codex sandbox TLS override. See docs/live-checks/godot-codex.md.\n'
      printf '[network]\n\ntls/certificate_bundle_override="%s"\n' "$CERT"
    } > override.cfg
  elif ! grep -q 'tls/certificate_bundle_override' override.cfg; then
    die "override.cfg exists without tls/certificate_bundle_override; add it under [network] with \"$CERT\""
  fi
}

# Count the sandbox lines (E1 to E9 of docs/live-checks/godot-codex.md): E3,
# or an E line that names a path outside the project.
count_sandbox_lines() {
  local log=$1 root=$2 real=$3 n=0 line path outside
  local e_lines='Could not create directory|Error attempting to create data dir|Could not open .user://. directory|Cannot save file|Error saving editor settings|Failed to open log file for writing|Cannot remove file or directory'
  while IFS= read -r line; do
    case "$line" in
      *'Condition "ret != noErr" is true'*) n=$((n + 1)); continue ;;
    esac
    printf '%s\n' "$line" | grep -Eq "$e_lines" || continue
    outside=0
    while IFS= read -r path; do
      path="${path#?}"
      case "$path" in
        "$root"|"$root"/*|"$real"|"$real"/*) ;;
        *) outside=1 ;;
      esac
    done < <(printf '%s\n' "$line" | grep -oE "(^|[ '\"])/[^'\"]*" || true)
    if [ "$outside" -eq 1 ]; then n=$((n + 1)); fi
  done < <(grep -E '^ERROR:' "$log" || true)
  printf '%s\n' "$n"
}

[ -f ./project.godot ] || die "no ./project.godot; run from the project root"

if is_csharp; then
  ENGINE="$(find_engine)" || die "C# project: no Godot .NET engine (GODOT_MONO_PATH, $MONO_APP); the standard engine cannot run C#"
else
  ENGINE="$(find_engine)" || die "no Godot engine (GODOT_PATH, godot on PATH, /Applications/Godot.app)"
fi

ROOT="$PWD"
REAL="$(pwd -P)"
H="$ROOT/.godot/horch-home"
export HOME="$H"
export XDG_DATA_HOME="$H/xdg/data"
export XDG_CONFIG_HOME="$H/xdg/config"
export XDG_CACHE_HOME="$H/xdg/cache"
mkdir -p "$H/logs" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"

if [ -n "${CODEX_SANDBOX:-}" ] && [ "$(uname)" = Darwin ]; then
  [ -r "$CERT" ] || die "$CERT is not readable"
  write_override
  version="$("$ENGINE" --headless --version 2>/dev/null | grep -Eo '^[0-9]+\.[0-9]+' | head -1 || true)"
  [ -n "$version" ] || die "cannot read the engine version from $ENGINE --version"
  write_editor_setting "$H/Library/Application Support/Godot/editor_settings-$version.tres"
fi

headless=0
for arg in "$@"; do
  if [ "$arg" = -- ]; then break; fi
  if [ "$arg" = --headless ]; then headless=1; fi
done
if [ "$headless" -eq 1 ]; then args=("$@"); else args=(--headless "$@"); fi

LOG="$H/logs/godot-run-$(date -u +%Y%m%dT%H%M%SZ)-$$.log"
set +e
"$ENGINE" "${args[@]}" 2>&1 | tee "$LOG"
code=${PIPESTATUS[0]}
set -e

n="$(count_sandbox_lines "$LOG" "$ROOT" "$REAL")"
if [ "$n" -gt 0 ]; then
  printf 'godot-run: SANDBOX: %s line(s); log: %s\n' "$n" "$LOG" >&2
  exit 3
fi
exit "$code"
