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
# parse. A failing block in a copied file (a `copied_files` path in
# skills/copied.json) is reported, not failed; so is a deprecated
# API name in such a file (GW13). Both Godot checks print "skipped: no Godot"
# and pass when Godot is not installed. xref_check needs no Godot: every
# godot-<name> mention names a skill and every relative link resolves.
# csharp_blocks_check compiles every csharp block with dotnet and
# Godot.NET.Sdk (same --strict-own rule; "skipped: no dotnet" without dotnet;
# 24 s on the 64 skills with a warm NuGet cache in .worktrees/_scratch).
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
step cargo fmt --all --check
step cargo build --workspace --all-targets
step cargo build --workspace --bins
step cargo clippy --workspace --all-targets -- -D warnings
# N-12: a rustdoc warning (a link to a private or renamed item) fails too.
step env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
step cargo test --workspace --no-fail-fast
step env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check
# No arguments: every requirement ID in docs/specs/*.md needs a test.
step scripts/check-req-coverage.sh
step scripts/check-deps.sh
step scripts/verify-telemetry-e2e.sh
step godot_skills
echo "GATE GREEN"
