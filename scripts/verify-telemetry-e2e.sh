#!/usr/bin/env bash
# The hermetic end-to-end story of the telemetry design (section 16.5): the
# real horch binary, fake harnesses first on PATH, a fresh temp state dir.
# Prints `PASS <step>` per step, then the rendered frame, or stops with the
# difference. The steps live in crates/horch-e2e/tests/scenario.rs.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --quiet --workspace --bins
cargo test --quiet -p horch-e2e --test scenario -- --nocapture --test-threads=1
echo "verify-telemetry-e2e: all steps passed"
