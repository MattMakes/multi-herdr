#!/usr/bin/env bash
# NFR-08: the per-commit gate for the arch-refactor-dataset branch.
# Runs every check in order and stops at the first failure. HORCH_REQUIRE_GIT
# and HORCH_REQUIRE_SQLITE pass through unchanged; set both to 1 on the Mac.
set -euo pipefail
cd "$(dirname "$0")/.."

# No inherited variable may aim a git call at a repository. On 2026-10-03 a
# gate run under `git rebase -x` inherited GIT_DIR, and test fixtures that
# `git init` and commit wrote into the real repository (core.bare=true,
# HEAD=main, 2 commits). The same list is REPO_ENV in
# crates/horch-marketplace/src/git.rs, with the numbered GIT_CONFIG_KEY_<n>
# and GIT_CONFIG_VALUE_<n>. Rebase first, then run the gate; never run it
# under `git rebase -x`.
unset GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CEILING_DIRECTORIES GIT_COMMON_DIR \
  GIT_CONFIG GIT_CONFIG_COUNT GIT_CONFIG_PARAMETERS GIT_DIR GIT_GRAFT_FILE \
  GIT_IMPLICIT_WORK_TREE GIT_INDEX_FILE GIT_INTERNAL_SUPER_PREFIX \
  GIT_NAMESPACE GIT_NO_REPLACE_OBJECTS GIT_OBJECT_DIRECTORY GIT_PREFIX \
  GIT_REPLACE_REF_BASE GIT_SHALLOW_FILE GIT_WORK_TREE
for v in $(compgen -e | grep -E '^GIT_CONFIG_(KEY|VALUE)_' || true); do
  unset "$v"
done

# No python step (and no cargo test that runs python) writes __pycache__
# beside the sources: build.rs bundles files under skills/ from disk.
export PYTHONDONTWRITEBYTECODE=1

step() {
  echo "==> gate: $*"
  "$@"
}

# No unresolved spec marker may remain: the spec-todo token (upper case) in
# any tracked file, or a Markdown line that starts with a pending placeholder
# (upper case, then a colon). U-07: 2 such appendices escaped a scan for the
# spec-todo token only. Prose that names the
# marker inside a line does not match. Each token is built from two parts
# so that this file does not match itself. nfr.rs runs this function on a
# scratch repository (nfr_08_marker_scan_*).
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

# Godot skills: every engine API name in
# skills/godot-* must exist in Godot 4.7.2, and every gdscript block must
# parse. A failing block or a deprecated API name (GW13) in a copied file (a
# `copied_files` path in skills/copied.json) does not fail by itself: the
# checks count them per skill, and a count above its baseline in
# scripts/godot/copied-baseline.json fails the step (U-22, copied_ratchet.py).
# Both Godot checks print "skipped: no Godot"
# and pass when Godot is not installed. xref_check needs no Godot: every
# godot-<name> mention names a skill and every relative link resolves.
# csharp_blocks_check compiles every csharp block with dotnet and
# Godot.NET.Sdk (same --strict-own rule; "skipped: no dotnet" without dotnet;
# 24 s on the 65 skills with a warm NuGet cache in .worktrees/_scratch).
# The unit tests also run the 4 GW9 gameplay scenarios headless (U-25,
# gameplay_scenarios.py).
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

# skill-intake: the `intake` tool's unittest suite (stdlib only, no network;
# every test sets HOME and XDG_STATE_HOME to a temp dir). Each test module
# runs in its own process, 8 at a time (U7: 1 process took 138 to 265 s).
# A failed module prints its log, and the step fails.
skill_intake_tests() {
  local logs
  logs=$(mktemp -d "${TMPDIR:-/tmp}/gate-intake.XXXXXX")
  local status=0
  (cd scripts/skill-intake/tests && ls test_*.py) | sed 's/\.py$//' |
    xargs -P 8 -I{} sh -c '
      cd scripts/skill-intake/tests || exit 255
      if python3 -m unittest "$1" >"$2/$1.log" 2>&1; then
        echo "ok: $1"
      else
        echo "FAILED: $1"
        cat "$2/$1.log"
        exit 1
      fi' sh {} "$logs" || status=1
  rm -rf "$logs"
  return "$status"
}

# The base for "changed" checks: GATE_BASE when set, else the fork point
# from origin/main. With no base, every optional suite runs.
gate_base() {
  if [ -n "${GATE_BASE:-}" ]; then
    echo "$GATE_BASE"
  else
    git merge-base HEAD origin/main 2>/dev/null || true
  fi
}

# Exit 0 when the path differs from the base (commits or working tree).
changed() {
  local base
  base=$(gate_base)
  [ -z "$base" ] || ! git diff --quiet "$base" -- "$1"
}

# hark: the engine pytest suite (Q2), only when skills/hark/ changed. It
# needs the hark venv (HARK_PYTHON, default ~/.local/state/hark/venv) with
# pytest. Without it the step prints a skip line and passes. Each test file
# runs in its own pytest process, 8 at a time: 1 process took 1172 s. Exit
# 5 (every test in the file deselected) passes.
hark_tests() {
  if ! changed skills/hark/; then
    echo "skipped: skills/hark/ did not change"
    return 0
  fi
  local py="${HARK_PYTHON:-$HOME/.local/state/hark/venv/bin/python}"
  if [ ! -x "$py" ] || ! "$py" -c "import pytest" 2>/dev/null; then
    echo "skipped: no hark venv with pytest at $py"
    return 0
  fi
  local logs
  logs=$(mktemp -d "${TMPDIR:-/tmp}/gate-hark.XXXXXX")
  local status=0
  (cd skills/hark/engine && find tests -name 'test_*.py' | sort) |
    xargs -P 8 -I{} sh -c '
      cd skills/hark/engine || exit 255
      log="$3/$(echo "$1" | tr / _).log"
      PYTHONPATH=. "$2" -m pytest "$1" -q -p no:cacheprovider \
        -m "not ml and not live" >"$log" 2>&1
      code=$?
      if [ "$code" -eq 0 ] || [ "$code" -eq 5 ]; then
        echo "ok: $1"
      else
        echo "FAILED: $1"
        cat "$log"
        exit 1
      fi' sh {} "$py" "$logs" || status=1
  rm -rf "$logs"
  return "$status"
}

# one-page-design: the check_page unittest suite (Q2), only when
# skills/one-page-design/ changed.
one_page_design_tests() {
  if ! changed skills/one-page-design/; then
    echo "skipped: skills/one-page-design/ did not change"
    return 0
  fi
  python3 -m unittest discover -s skills/one-page-design/scripts/tests
}

step no_spec_todo
step cargo fmt --all --check
step cargo build --workspace --all-targets
step cargo build --workspace --bins
step cargo clippy --workspace --all-targets -- -D warnings
# Code under cfg(debug_assertions) hides release-only warnings from the
# debug build above; `just install` builds release.
step cargo clippy --workspace --bins --release -- -D warnings
# N-12: a rustdoc warning (a link to a private or renamed item) fails too.
step env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
step cargo test --workspace --no-fail-fast
step env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check
# No arguments: every requirement ID in docs/specs/*.md needs a test.
step scripts/check-req-coverage.sh
step scripts/check-deps.sh
step scripts/verify-telemetry-e2e.sh
step skill_intake_tests
step hark_tests
step one_page_design_tests
step godot_skills
echo "GATE GREEN"
