#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

unset GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CEILING_DIRECTORIES GIT_COMMON_DIR \
  GIT_CONFIG GIT_CONFIG_COUNT GIT_CONFIG_PARAMETERS GIT_DIR GIT_GRAFT_FILE \
  GIT_IMPLICIT_WORK_TREE GIT_INDEX_FILE GIT_INTERNAL_SUPER_PREFIX \
  GIT_NAMESPACE GIT_NO_REPLACE_OBJECTS GIT_OBJECT_DIRECTORY GIT_PREFIX \
  GIT_REPLACE_REF_BASE GIT_SHALLOW_FILE GIT_WORK_TREE
for v in $(compgen -e | grep -E '^GIT_CONFIG_(KEY|VALUE)_' || true); do
  unset "$v"
done

step() {
  echo "==> gate: $*"
  "$@"
}

no_spec_todo() {
  local hits token="SPEC-""TODO" pending="^[[:space:]]*PEND""ING:"
  hits=$({
    git grep -n "$token" -- .
    git grep -nE "$pending" -- '*.md'
  } || true)
  if [ -n "$hits" ]; then
    echo "unresolved spec markers found (close them):" >&2
    echo "$hits" >&2
    return 1
  fi
}

comment_ban() {
  python3 -m unittest discover -s scripts/comments/tests
  python3 scripts/comments/comment_lint.py check
}

godot_skills() {
  python3 -m unittest discover -s scripts/godot/tests
  local dirs=(skills/godot-*/)
  if [ ! -d "${dirs[0]}" ]; then
    echo "skipped: no skills/godot-*"
    return 0
  fi
  python3 scripts/godot/xref_check.py
  python3 scripts/godot/api_check.py --strict-own skills/copied.json "${dirs[@]}"
  python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/copied.json "${dirs[@]}"
  python3 scripts/godot/csharp_blocks_check.py --strict-own skills/copied.json "${dirs[@]}"
}

step no_spec_todo
step comment_ban
step cargo fmt --all --check
step cargo build --workspace --all-targets
step cargo build --workspace --bins
step cargo clippy --workspace --all-targets -- -D warnings
step cargo clippy --workspace --bins --release -- -D warnings
step env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
step cargo test --workspace --no-fail-fast
step env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check
step scripts/check-req-coverage.sh
step scripts/check-deps.sh
step scripts/verify-telemetry-e2e.sh
step python3 -m unittest discover -s scripts/skill-intake/tests
step godot_skills
echo "GATE GREEN"
