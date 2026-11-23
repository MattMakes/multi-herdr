# Fix tickets index: end reviews R1 and R2

Reviews covered:
- R1: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (on `fleet/fleetd-finish`)
- R2: `docs/specs/fleetd/reviews/end-review-p2-p3.md` (on `fleet/fleetd-finish`)

Note: R1-F5 (cross-platform CI proof for PL-1, PL-3, PL-6) is omitted as instructed because the CI proof has its own tickets (`23-ci0-early-ci.md` and `32-e5-draft-pr-and-ci.md`).

## Fix tickets

| File | Finding ID | Rank | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [35-fix-r1-f1-complexity-sweep-sandbox.md](35-fix-r1-f1-complexity-sweep-sandbox.md) | R1-F1 | Critical | primary `opus-hardening`, fallback `opus` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/tests/check_sandbox.rs`, `crates/horch-core/tests/verifier_review.rs` |  |
| [36-fix-r1-f2-windows-channel-bounded-write.md](36-fix-r1-f2-windows-channel-bounded-write.md) | R1-F2 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-platform/src/channel.rs`, `crates/horch-platform/src/windows/channel.rs`, `crates/horch-platform/tests/channel.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs` |  |
| [37-fix-r1-f3-pass-loop-event-wake.md](37-fix-r1-f3-pass-loop-event-wake.md) | R1-F3 | Important | primary `opus`, fallback `codex-sol` | `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/tests/p1_pass.rs` |  |
| [38-fix-r1-f4-integration-per-row-savepoint.md](38-fix-r1-f4-integration-per-row-savepoint.md) | R1-F4 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/integration/application/features.rs`, `crates/horch-core/src/fleetd/integration/application/landing.rs`, `crates/horch-core/tests/feature_states.rs`, `crates/horch-core/tests/landing_states.rs` |  |
| [39-fix-r2-f1-triage-apply-per-decision-savepoint.md](39-fix-r2-f1-triage-apply-per-decision-savepoint.md) | R2-F1 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/tests/triage_apply_job.rs` |  |

## File overlaps and `pass.rs` avoidance

- **`pass.rs` avoidance**: None of tickets 35 through 39 touches `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`. In particular, ticket 38 (R1-F4) places the per-feature and per-branch savepoints inside `crates/horch-core/src/fleetd/integration/application/features.rs` (`follow_in`) and `crates/horch-core/src/fleetd/integration/application/landing.rs` (`follow`), keeping the fix out of `pass.rs` so it is not blocked by the landing chain (T-15).
- **Shared file notice (`jobs.rs`)**: Both R1-F1 (`jobs.rs:1132`) and R2-F1 (`jobs.rs:881`) mention `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`. To prevent an `Own` path conflict between ticket 35 and ticket 39, ticket 35 owns `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` and ticket 39 places its per-decision savepoint inside `crates/horch-core/src/taskledger/triage/apply.rs` (`ingest` at line 1661, which `jobs::record_facts` calls). No two tickets in this set share an `Own` path.
- **Note on ticket 02 (A2)**: Ticket 39 (`39-fix-r2-f1-triage-apply-per-decision-savepoint.md`) owns `crates/horch-core/src/taskledger/triage/apply.rs`, which was also owned by ticket 02 (`02-a2-triage-detached.md`, already landed on `fleet/fleetd-finish`).

## Minor findings

- **R1 (`end-review-p0-pp-p1.md`)**: None (0 Minor findings).
- **R2 (`end-review-p2-p3.md`)**: None (0 Minor findings).

## Open questions

1. **R1 operator decision — Calendar-time injection for rescue-ref retention (watchlist line 45, T-399)**: `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4690` passes `chrono::Utc::now()` to `rescue_refs::collect` (calendar time under PL-4, not the fleet clock). R1 notes this is not a design fault and recommends a calendar-clock port only if deterministic rescue-retention unit injection is desired. Left untouched.
2. **R2 operator decision 1 — Manual resume under push dispatch (`g52_resume_uses_own_session`, watchlist line 36, T-881 / T-882)**: R2 notes that manual session resume races push dispatch and recommends retiring `horch spawn --resume` for fleet-managed worker tasks (ticket 15 / C12 T-882 already owns `fleet18_observability.txt` and resume under push dispatch).
3. **R2 operator decision 2 — Repair with no worktree row (watchlist line 51, T-397)**: R2 confirms no silent stall exists (a missing worktree opens a visible `repair_start` item and offers branch abandonment) and recommends treating it as an invariant breach rather than adding automatic worktree reconstruction.
4. **R2 operator decision 3 — Fetch process hang (watchlist line 72, T-397)**: R2 notes that the reported 1-of-4 fetch process hang is in the integration job executor outside Phase 2/3 code and recommends assigning the integration/job review to verify bounded timeout and git-writer queue release when a project has no origin.
