---
name: trace
description: Use when tracing an endpoint, function, event, job, or comparing behavior across old and new implementations.
---

# Trace

Start from the entry point named in the brief: HTTP method/path, file and symbol, handler registration, CLI command, or scheduled job. If ambiguous, discover plausible entry points and resolve only the ambiguity that blocks the trace.

1. Detect the language and framework from project manifests. Locate the actual route, symbol, or registration. Prefer available definition/reference navigation; fall back to targeted text search without requiring a plugin installation.
2. Trace upstream callers, middleware, dispatch, scheduling, and construction/injection. Record what triggers execution and the applicable authentication, authorization, and validation guards.
3. Follow downstream application calls through services and data access, reading implementations. Record branch conditions, transformations, exact defaults, state changes, database operations, external requests, and emitted events. Stop at standard-library or third-party boundaries after documenting their inputs and relevant semantics.
4. Follow error and cancellation paths as carefully as success paths: caught/propagated errors, status/body shape, retries, timeouts, locking, transactions, and cleanup. Resolve similarly named symbols by imports and types rather than guessing.
5. Write `ai_docs/trace/topic.md` or the assigned path with trigger, execution flow, business rules, side effects, outputs/errors, upstream callers, and source citations. Omit inapplicable sections; mark dynamic behavior that cannot be established statically.

For parity work, trace both implementations independently. Compare contracts, indicator values/types, hashing, credentials, serialization, concurrency, retries, caching, and side effects. Record each aspect as MATCH, BREAK, MISSING, or NEW with evidence from both sides. Distinguish intended differences already authorized in the brief from unexplained drift. Report drift to the orchestrator; do not silently change either implementation during a trace.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
