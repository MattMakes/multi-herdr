#!/usr/bin/env bash
# NFR-08: the per-commit gate for the arch-refactor-dataset branch
# (ai_docs/plans/arch-refactor-dataset/00-master-plan.md, section 5).
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

# No unresolved spec marker may remain. The plan and conventions files of the
# finish run describe the token, so they are excluded by path. The token is
# built from two parts so that this file does not match itself.
no_spec_todo() {
  local hits token="SPEC-""TODO"
  hits=$(git grep -n "$token" -- . \
    ':(exclude)ai_docs/plans/finish/t6-spec-history.md' \
    ':(exclude)ai_docs/plans/finish/00-conventions.md' || true)
  if [ -n "$hits" ]; then
    echo "unresolved spec markers found (close them; see ai_docs/plans/finish/00-conventions.md):" >&2
    echo "$hits" >&2
    return 1
  fi
}

# Godot skills (ai_docs/plans/godot/gw0-tooling.md): every engine API name in
# skills/godot-* must exist in Godot 4.7.2, and every gdscript block must
# parse. A failing block in a file copied from GodotPrompter (a `sources`
# path in skills/provenance.json) is reported, not failed. Both checks print
# "skipped: no Godot" and pass when Godot is not installed.
godot_skills() {
  python3 -m unittest discover -s scripts/godot/tests
  local dirs=(skills/godot-*/)
  if [ ! -d "${dirs[0]}" ]; then
    echo "skipped: no skills/godot-*"
    return 0
  fi
  python3 scripts/godot/api_check.py "${dirs[@]}"
  python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/provenance.json "${dirs[@]}"
}

step no_spec_todo
step cargo fmt --all --check
step cargo build --workspace --all-targets
step cargo build --workspace --bins
step cargo clippy --workspace --all-targets -- -D warnings
step cargo test --workspace --no-fail-fast
step env HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check
# No arguments: the coverage check reads ai_docs/gates/architecture-refactor/CURRENT_PHASE.
step scripts/check-req-coverage.sh
step scripts/check-deps.sh
step scripts/verify-telemetry-e2e.sh
step godot_skills
echo "GATE GREEN"
