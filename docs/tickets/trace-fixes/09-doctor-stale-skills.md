# 09: doctor warns about stale ambient herdr skills

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch -- doctor trf_10`
**Own:** `crates/horch/src/cmd/doctor.rs`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Tell the operator when `~/.claude/skills/herdr-*` links shadow the compiled briefing (D18).

## Requirements

- **TRF-10**: `horch doctor` warns about `~/.claude/skills/herdr-*` links that are not the compiled briefing. Test name starts with `trf_10_`.

## Steps

1. Add a doctor warning for each `herdr-*` entry under the user's Claude skills folder; name it and say the compiled briefing is canonical.
2. Read the home folder through the existing runtime path helpers, not a direct env read (arch_scan rule).
3. Do not delete anything.

## Done when

TRF-10 test passes with a temp home folder.

## Report

Write `ai_docs/reports/trace-fixes/09.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
