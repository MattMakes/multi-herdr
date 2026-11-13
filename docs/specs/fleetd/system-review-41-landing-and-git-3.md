# Systems review 41, round 3: landing, git and no-loss rules

Scope: `docs/specs/fleetd/design.md` at `5327458bee4a1ab135e4dc2b38db02fb0f75f845`, compared with `c8bf35ac2083746a95414f24cc3679bf2a16f1d6`.

## Verdict

PASS

No HIGH finding is open.

## Answers

- T41-1 (CRITICAL) closed: a compare-and-set mismatch moves the `passed` bundle to `dropped`, returns its members to `ready`, records the new tip and rebuilds from it (`design.md:846`, `design.md:1128`, `design.md:1137`).
- T41-2 (HIGH) closed: fleetd owns each candidate ref and deletes it after the bundle is `landed`, `failed` or `dropped` and no child bundle is active; fleetd verifies and retries deletion (`design.md:700`, `design.md:703`, `design.md:1135`).
- T41-3 (HIGH) closed: effect jobs never reuse results, each run has its own run number, failed pushes queue a new run after backoff, and each fetch period creates a new run (`design.md:751`, `design.md:1101`, `design.md:1183`, `design.md:1295-1307`).

## New CRITICAL

None
