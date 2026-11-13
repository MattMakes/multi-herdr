# Systems review 42, round 2: migration plan and requirements

Scope: `docs/specs/fleetd/design.md` at `c8bf35ac2083746a95414f24cc3679bf2a16f1d6`, compared with the first review at `62337c29`.

## Verdict

PASS

The amendment closes S42-1, the only prior CRITICAL finding, and introduces no new CRITICAL finding. 3 HIGH findings remain open: S42-2, S42-4, and T42-1.

## Answers

- S42-1 closed
- S42-2 open - Phase 1 to Phase 4 command compatibility requires direct ledger writes, but the architecture still says agents never touch the ledger.
- S42-3 closed
- S42-4 open - FM-9 is marked green in Phase 4 although its paired-judge path and RR-3 become green only in Phase 6.
- S42-5 closed
- S42-6 closed
- S42-7 decision

## New findings

### T42-1 - HIGH - Peer-user checks do not separate workers from operator commands

Status: CONFIRMED.

Lines: `design.md:179`, `338`, `1550`, `1566`.

> A worker cannot send an operator command: `horch idea approve`, `abandon` and `horch idea add --direct` refuse to run inside a fleet attempt.

Failure scenario: Phase 5 authenticates a channel request only by the ledger owner's OS user, and authenticates a request file by owner and mode. A worker process runs for that same user, and the design assigns it no separate channel identity or operator capability. The worker can bypass the `horch` command refusal by sending the protocol request directly or creating a same-owner request file if its sandbox exposes that endpoint. The request then passes the stated checks and can approve an idea, abandon work, or invoke another state-changing operator action. A builder must guess whether every worker sandbox blocks both endpoints, and a wrong rule can authorize a destructive request.

Smallest change: give operator requests a capability that fleetd never places in a worker profile or attempt. Require fleetd to authorize the request kind against that capability, and add a Phase 5 test where a worker sends a raw channel request and a same-owner request file for each operator-only action.
