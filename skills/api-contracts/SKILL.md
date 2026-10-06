---
name: api-contracts
description: Use when building or changing a service boundary - HTTP/REST, RPC, a queue consumer, a webhook, a CLI or library entry point - to pin down the contract. Covers where input is validated, the error model (retryable, caller mistake, page a human), idempotency and retries, ordering, pagination, timeouts, partial failure and compatible evolution. Language-neutral; follow the repo's own framework and idioms.
---

# API contracts

Make a boundary say exactly what it does: what it accepts, what it returns, how it fails, and what it promises when called twice, late or concurrently. Most production bugs at a boundary are a contract that nobody wrote down. Write it down, then make the code and the tests keep it.

This skill is language-neutral. Use the framework, error types and validation library the repository already uses. Look up library behaviour with the docs tool you have (for example context7); do not recall it.

## When to use

- A new endpoint, RPC method, message handler, webhook, job, CLI command or public library function.
- A change to the request, response, error or side effects of an existing one.
- A bug report that is really a contract gap: a duplicate charge, a silent drop, a 500 for bad input, a client that broke after a deploy.

## Workflow

1. **Find the existing conventions.** Before you design anything, read two or three neighbouring endpoints or handlers. Note: the error envelope, the validation layer, the auth check, the pagination style, the ID format, the time format, how idempotency keys work (if they do), how handlers are tested. Check: you can name the file that your new code should look like.

2. **Write the contract before the code.** In the PR description, a doc comment or the API schema file the repo uses (OpenAPI, protobuf, GraphQL SDL, JSON Schema), state:
   - the operation and who may call it (authn, authz);
   - each input field: type, required or optional, bounds, format, default;
   - the success result, including what is guaranteed to be persisted when the call returns;
   - each error, using the error model in step 4;
   - idempotency, ordering and concurrency behaviour (steps 5 and 6);
   - limits: payload size, page size, rate, timeout.
   Check: a client author could write a correct client from this alone.

3. **Validate at the edge, once.** Parse untrusted input into a typed value at the boundary, and pass only the typed value inward. Reject unknown or malformed input with a caller-mistake error that names the field and the rule. Do not re-validate the same rule deep inside; do not let a raw request object reach domain code. Check: there is a test for each rejection rule, and each one returns the field name.

4. **Design the error model on purpose.** Every failure is one of three kinds. Decide which, for each case:

   | Kind | Meaning | HTTP (typical) | Caller should | Log level |
   |---|---|---|---|---|
   | Caller mistake | input, auth or state the caller can fix | 400, 401, 403, 404, 409, 422 | fix and not retry as is | info/debug |
   | Retryable | transient: timeout, overload, a dependency down, a lost lock | 429, 503 (with `Retry-After`), 504 | retry with backoff, same idempotency key | warn |
   | Bug or invariant broken | the service is wrong; a human must look | 500 | not retry blindly | error, with context, alerting |

   Rules:
   - Keep the repo's error envelope. Each error has a stable machine code (`"insufficient_funds"`), a human message, and the field when there is one.
   - Never put stack traces, SQL, internal hostnames or secrets in a response.
   - Map dependency errors at the boundary. A vendor's error type does not cross into your domain or out to your caller.
   - Swallowing an error is a decision. If you catch and continue, the code comment says why, and a metric or log line records it.
   - A log line for a failure carries enough to diagnose it alone: operation, IDs, the input that matters (not secrets), the dependency, the elapsed time.
   Check: each documented error has a test that triggers it and asserts the code and status.

5. **Make retries safe: idempotency.** Any operation that has a side effect and can be retried (by a client, a proxy, a queue redelivery, a job restart) must be idempotent or detect duplicates.
   - Naturally idempotent: `PUT` of a full resource, `DELETE`, "set X to Y".
   - Not naturally idempotent: create, charge, send, increment, append. Use an idempotency key from the caller, or a natural unique key, stored with a unique constraint in the same transaction as the effect. A repeat with the same key returns the first result; a repeat with the same key and a different body is a caller mistake (409 or 422).
   - Message consumers: assume at-least-once delivery. Record processed message IDs, or make the handler's effect idempotent.
   Check: a test sends the same request twice (and twice concurrently, where the framework allows) and asserts one effect.

6. **State ordering and concurrency.** Say what is guaranteed and what is not:
   - Concurrent updates to one resource: last write wins, optimistic concurrency (version or ETag with `If-Match`, 409/412 on conflict), or a lock. Pick one and say which.
   - Event or message order: per key, per partition, or none. A consumer that needs order and does not have it must handle out-of-order and gaps.
   - Read-after-write: does a read right after a successful write see it (a replica, a cache, a search index)? If not, say so in the contract.

7. **Handle partial failure.** For each operation that touches more than one store or service, list the steps and answer: if the process dies after step N, what state is left, and what makes it consistent? Use one of: a single transaction; the outbox pattern (write the event to a table in the same transaction, publish after); a saga with compensation; a reconciliation job. "It will not crash there" is not an answer. Check: the answer is in a code comment or the design doc, and there is a test for the most likely partial failure.

8. **Bound everything.** Every outbound call has a timeout shorter than the caller's. Every list has a maximum page size. Every request body has a size limit. Every retry loop has a maximum count and backoff with jitter. Every queue consumer has a dead-letter path or a poison-message rule.

9. **Paginate lists from the start.** Use the repo's style. If there is none, prefer cursor pagination (an opaque cursor over a stable sort key with a unique tiebreaker) over offset for data that changes. Return the next cursor, not a total count, unless the count is cheap and needed.

10. **Evolve compatibly.** A change to a published contract is either additive or versioned.
    - Safe: a new optional input field, a new output field, a new endpoint, a new error code that clients already treat as a generic error class.
    - Breaking: removing or renaming a field, changing a type or a format, making an optional field required, changing a default, changing the meaning of a status code, tightening validation on existing input.
    - For a breaking change: add the new shape beside the old one, migrate callers, measure that the old one has no traffic, then remove it. Say this order in the PR.
    - Clients must ignore unknown fields; servers must not depend on clients doing anything they did not do before.
    Check: the contract tests for the old shape still pass, or the PR says which callers were migrated.

11. **Test the contract, not the implementation.** Handler tests at the boundary for: each validation rule, each error kind, the idempotent repeat, the concurrency conflict, the page boundary, the timeout path of each dependency (with a fake that stalls). Use `tdd` for the red/green loop.

## Review checklist

- [ ] The contract is written down in the repo's schema or doc format.
- [ ] Input is parsed into types at the boundary; each rejection names the field and has a test.
- [ ] Every error is classified as caller mistake, retryable or bug, with a stable code and a test.
- [ ] No internal detail leaks in errors; dependency errors are mapped at the boundary.
- [ ] Side-effecting operations are idempotent or deduplicated, with a repeat test.
- [ ] Concurrency, ordering and read-after-write behaviour are stated.
- [ ] Each multi-step write has a stated recovery for a crash between steps.
- [ ] Timeouts, size limits, page limits and retry caps exist.
- [ ] Contract changes are additive, or versioned with a migration order.
