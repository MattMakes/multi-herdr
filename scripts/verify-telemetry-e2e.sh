#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --quiet --workspace --bins
cargo test --quiet -p horch-e2e --test scenario -- --nocapture --test-threads=1
echo "verify-telemetry-e2e: all steps passed"
