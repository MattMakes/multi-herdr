#!/usr/bin/env bash
# Compare plain and workspace-local Godot state in a Codex sandbox.
set -u

repo=$(cd "$(dirname "$0")/../.." && pwd)
work="$repo/.worktrees/_scratch/godot-codex-trial"
godot=${GODOT_PATH:-/Applications/Godot.app/Contents/MacOS/Godot}
failed=0

pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; failed=1; }
skip() { printf 'SKIP %s: %s\n' "$1" "$2"; }
bounded() { local seconds=$1; shift; perl -e 'alarm shift; exec @ARGV' "$seconds" "$@"; }

if [ ! -x "$godot" ]; then
  skip godot "no executable at '$godot' (set GODOT_PATH)"
  exit 0
fi

if [ -n "${CODEX_SANDBOX:-}" ]; then
  pass "sandbox-context CODEX_SANDBOX=$CODEX_SANDBOX"
else
  printf 'NOTE sandbox-context: CODEX_SANDBOX is not set; this run does not prove Codex workspace-write behavior.\n'
fi

rm -rf "$work"
mkdir -p "$work/logs/base" "$work/logs/fixed" \
  "$work/.home/home" "$work/.home/xdg-data" \
  "$work/.home/xdg-config" "$work/.home/xdg-cache" \
  "$work/.outside/home" "$work/.outside/xdg-data" \
  "$work/.outside/xdg-config" "$work/.outside/xdg-cache"

cat > "$work/project.godot" <<'EOF'
config_version=5

[application]
config/name="Godot Codex Trial"
run/main_scene="res://main.tscn"
config/features=PackedStringArray("4.7")

[display]
window/size/viewport_width=320
window/size/viewport_height=180

[rendering]
renderer/rendering_method="gl_compatibility"
renderer/rendering_method.mobile="gl_compatibility"
EOF

cat > "$work/main.tscn" <<'EOF'
[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node"]
script = ExtResource("1_main")
EOF

cat > "$work/main.gd" <<'EOF'
extends Node


func _ready() -> void:
	print("GODOT_CODEX_MAIN_READY")
EOF

cat > "$work/.home/parse_check.gd" <<'EOF'
extends SceneTree

const EXTENSIONS: PackedStringArray = ["gd", "tscn", "tres"]

var _checked := 0
var _failed: PackedStringArray = []


func _init() -> void:
	_walk("res://")
	for path in _failed:
		print("PARSE_CHECK FAIL ", path)
	print("PARSE_CHECK checked=%d failed=%d" % [_checked, _failed.size()])
	quit(1 if not _failed.is_empty() else 0)


func _walk(dir_path: String) -> void:
	if FileAccess.file_exists(dir_path.path_join(".gdignore")):
		return
	for dir_name in DirAccess.get_directories_at(dir_path):
		if dir_name.begins_with("."):
			continue
		_walk(dir_path.path_join(dir_name))
	for file_name in DirAccess.get_files_at(dir_path):
		if file_name.get_extension() in EXTENSIONS:
			_check(dir_path.path_join(file_name))


func _check(path: String) -> void:
	_checked += 1
	var res := ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_IGNORE)
	if res == null:
		_failed.append(path)
	elif res is Script and not (res as Script).can_instantiate():
		_failed.append(path)
EOF

error_pattern='SCRIPT ERROR:|ERROR:|Operation not permitted|operation not permitted|deny\('

run_step() {
  local mode=$1
  local step=$2
  local log=$3
  shift 3
  bounded 180 "$@" > "$log" 2>&1
  local code=$?
  local first_error
  first_error=$(grep -E -m1 "$error_pattern" "$log" || true)
  if [ "$code" -ne 0 ]; then
    fail "$mode-$step" "exit $code${first_error:+; $first_error}; log ${log#$repo/}"
  elif [ -n "$first_error" ]; then
    fail "$mode-$step" "exit 0; $first_error; log ${log#$repo/}"
  else
    case "$step" in
      version)
        grep -q '^4\.7\.2\.stable\.official\.' "$log" || {
          fail "$mode-$step" "exit 0; unexpected version; log ${log#$repo/}"
          return
        }
        ;;
      import)
        [ -d "$work/.godot/imported" ] || {
          fail "$mode-$step" "exit 0; .godot/imported is missing; log ${log#$repo/}"
          return
        }
        ;;
      parse)
        grep -q '^PARSE_CHECK checked=[0-9][0-9]* failed=0$' "$log" || {
          fail "$mode-$step" "exit 0; parse summary is missing; log ${log#$repo/}"
          return
        }
        ;;
      run)
        grep -q '^GODOT_CODEX_MAIN_READY$' "$log" || {
          fail "$mode-$step" "exit 0; main scene marker is missing; log ${log#$repo/}"
          return
        }
        ;;
      doctool)
        local docs
        docs=$(find "$work/doctool-$mode" -type f -name '*.xml' | wc -l | tr -d ' ')
        [ "$docs" -gt 1000 ] || {
          fail "$mode-$step" "exit 0; generated $docs XML files; log ${log#$repo/}"
          return
        }
        ;;
    esac
    pass "$mode-$step exit=0"
  fi
}

run_mode() {
  local mode=$1
  local log_dir=$2
  local doc_dir="$work/doctool-$mode"
  shift 2
  mkdir -p "$doc_dir"
  run_step "$mode" version "$log_dir/$mode-version.log" "$@" "$godot" --headless --version
  run_step "$mode" import "$log_dir/$mode-import.log" "$@" "$godot" --headless --path "$work" --import
  run_step "$mode" parse "$log_dir/$mode-parse.log" "$@" "$godot" --headless --path "$work" --script "$work/.home/parse_check.gd"
  run_step "$mode" run "$log_dir/$mode-run.log" "$@" "$godot" --headless --path "$work" --quit-after 60
  run_step "$mode" doctool "$log_dir/$mode-doctool.log" "$@" "$godot" --headless --path "$work" --doctool "$doc_dir"
}

if [ -n "${CODEX_SANDBOX:-}" ]; then
  run_mode A "$work/logs/base"
else
  run_mode A "$work/logs/base" env \
    HOME="$work/.outside/home" \
    XDG_DATA_HOME="$work/.outside/xdg-data" \
    XDG_CONFIG_HOME="$work/.outside/xdg-config" \
    XDG_CACHE_HOME="$work/.outside/xdg-cache"
fi
run_mode B "$work/logs/base" env \
  HOME="$work/.home/home" \
  XDG_DATA_HOME="$work/.home/xdg-data" \
  XDG_CONFIG_HOME="$work/.home/xdg-config" \
  XDG_CACHE_HOME="$work/.home/xdg-cache"

cert=/etc/ssl/cert.pem
settings="$work/.home/home/Library/Application Support/Godot/editor_settings-4.7.tres"
if [ ! -r "$cert" ]; then
  fail B-fixed-setup "'$cert' is not readable"
elif [ ! -f "$settings" ]; then
  fail B-fixed-setup "Godot did not create '$settings'"
elif ! grep -q '^network/tls/editor_tls_certificates = ""$' "$settings"; then
  fail B-fixed-setup "editor TLS setting is missing from '$settings'"
else
  perl -0pi -e 's#network/tls/editor_tls_certificates = ""#network/tls/editor_tls_certificates = "/etc/ssl/cert.pem"#' "$settings"
  cat > "$work/override.cfg" <<'EOF'
[network]

tls/certificate_bundle_override="/etc/ssl/cert.pem"
EOF
  run_mode B-fixed "$work/logs/fixed" env \
    HOME="$work/.home/home" \
    XDG_DATA_HOME="$work/.home/xdg-data" \
    XDG_CONFIG_HOME="$work/.home/xdg-config" \
    XDG_CACHE_HOME="$work/.home/xdg-cache"
fi

cat > "$work/health.gd" <<'EOF'
class_name Health
extends Node

signal died

@export_range(1, 100000) var max_health: int = 100
var current_health: int = max_health


func damage(amount: int) -> void:
	if amount <= 0 or current_health == 0:
		return
	current_health = maxi(0, current_health - amount)
	if current_health == 0:
		died.emit()


func heal(amount: int) -> void:
	if amount <= 0 or current_health == 0:
		return
	current_health = mini(max_health, current_health + amount)
EOF

cat > "$work/test_health.gd" <<'EOF'
extends SceneTree


func _init() -> void:
	var failures: PackedStringArray = []
	var death_count := [0]
	var health := Health.new()
	health.died.connect(func() -> void: death_count[0] += 1)
	health.damage(25)
	_expect(health.current_health == 75, "damage subtracts health", failures)
	health.heal(10)
	_expect(health.current_health == 85, "heal restores health", failures)
	health.heal(1000)
	_expect(health.current_health == health.max_health, "heal clamps to max_health", failures)
	health.damage(health.max_health)
	_expect(health.current_health == 0, "lethal damage clamps to zero", failures)
	_expect(death_count[0] == 1, "lethal damage emits died once", failures)
	health.damage(1)
	health.heal(1)
	_expect(health.current_health == 0, "dead health cannot heal", failures)
	_expect(death_count[0] == 1, "damage after death does not emit died", failures)
	health.free()
	for failure in failures:
		push_error(failure)
	print("HEALTH_TEST passed=%d failed=%d" % [7 - failures.size(), failures.size()])
	quit(1 if not failures.is_empty() else 0)


func _expect(condition: bool, message: String, failures: PackedStringArray) -> void:
	if not condition:
		failures.append(message)
EOF

fixed_env=(env \
  HOME="$work/.home/home" \
  XDG_DATA_HOME="$work/.home/xdg-data" \
  XDG_CONFIG_HOME="$work/.home/xdg-config" \
  XDG_CACHE_HOME="$work/.home/xdg-cache")

bounded 180 "${fixed_env[@]}" "$godot" --headless --path "$work" --import > "$work/logs/fixed/health-import.log" 2>&1
health_import_code=$?
bounded 180 "${fixed_env[@]}" "$godot" --headless --path "$work" --script "$work/.home/parse_check.gd" > "$work/logs/fixed/health-parse.log" 2>&1
health_parse_code=$?
bounded 180 "${fixed_env[@]}" "$godot" --headless --path "$work" --script res://test_health.gd > "$work/logs/fixed/health-test.log" 2>&1
health_test_code=$?

health_error=$(grep -E -m1 "$error_pattern" "$work/logs/fixed/health-"*.log || true)
if [ "$health_import_code" -eq 0 ] && [ "$health_parse_code" -eq 0 ] && \
   [ "$health_test_code" -eq 0 ] && [ -z "$health_error" ] && \
   grep -q '^PARSE_CHECK checked=4 failed=0$' "$work/logs/fixed/health-parse.log" && \
   grep -q '^HEALTH_TEST passed=7 failed=0$' "$work/logs/fixed/health-test.log"; then
  pass "health-task import=0 parse=0 test=0; 7 of 7 assertions passed"
else
  fail health-task "import=$health_import_code parse=$health_parse_code test=$health_test_code${health_error:+; $health_error}; logs ${work#$repo/}/logs/fixed/health-*.log"
fi

rm -rf "$work/doctool-A" "$work/doctool-B" "$work/doctool-B-fixed"
exit "$failed"
