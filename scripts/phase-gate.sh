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
# crates/horch-marketplace/src/git.rs. Rebase first, then run the gate;
# never run it under `git rebase -x`.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR \
  GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NAMESPACE \
  GIT_CEILING_DIRECTORIES GIT_PREFIX

step() {
  echo "==> gate: $*"
  "$@"
}

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
echo "GATE GREEN"
