# Second system review 43: operations, trust, and document agreement

## Verdict

FAIL. The original CRITICAL finding is closed, but T43-1 is a new CRITICAL contradiction between the closed `jobs.kind` schema and the required docs-sync job.

0 HIGH findings from the first review remain open. This review found 2 new HIGH findings.

## Answers

- S43-1 (HIGH) closed: lines 180 and 420-429 require regular files opened without following links, path and payload binding to the owning attempt, per-type byte and line limits, and permanent rejection records. The oversized-file, symlink, and cross-attempt steps now stop before ingestion.
- S43-2 (CRITICAL) closed: lines 827, 833, and 957-959 make the live-process invariant feature-scoped. Feature F with ideas I1 and I2 now has 1 sub-orchestrator, while its ideas are planned in order and their tasks can run in parallel.
- S43-3 (MEDIUM) closed: lines 181, 222, 900, and 1326-1328 require every worker, verifier, job, docs-sync, version, and conformance process to use the single scrubbed launcher. Native herdr agent resume is prohibited.
- S43-4 decision.
- S43-5 (MEDIUM) closed: lines 198, 1260, and 1269 require status to show each dispatch hold and its cause, and require events when a global hold starts or ends.
- S43-6 (MEDIUM) closed: lines 200 and 998 give each needs-you item a stable key, lifecycle states, upsert behavior, open-only display, cause-based resolution, and 30-day terminal retention.
- S43-7 (MEDIUM) closed: lines 408-415 split immutable messages, replaceable snapshots, and append-only logs, so heartbeat replacement and progress appends no longer contradict message immutability.
- S43-8 (MEDIUM) closed: lines 45, 162-169, 291-299, 334-338, 546, 983-998, 1275-1278, and 1326 state the D15 request, refusal, quota, result-status, question-limit, requeue-counter, module-owner, supervision, and docs-sync rules in the design.

## New findings

### T43-1 - CRITICAL - The required docs-sync job has no valid jobs row

Lines: 142, 723-738, 1283-1330, 1552, 1607-1613.

> `('rebase','rescue','fetch','push')),`
> `Docs sync is a horch subcommand. fleetd schedules it as a job, so it runs outside the reconcile pass.`

Failure scenario: Phase 7 enables the daily docs sync or detects a changed CLI binary. fleetd must schedule docs sync as a job, but `jobs.kind` has no `docs_sync` value and the row also requires a repository. SQLite rejects the row, no conformance run occurs, and the required FR-8 mechanism cannot be built as written. The Phase 7 and New work tables add docs sync but name no schema change that makes this row valid.

Smallest change: add a Phase 7 migration that adds `docs_sync` to `jobs.kind`, defines its subject and nullable or applicable repository binding, and gives it an input key and time limit. Name that schema change in the Phase 7 and New work rows.

### T43-2 - HIGH - Same-user workers can bypass the operator-command check

Lines: 179, 332-338, 794-803, 1127-1132, 1550.

> `The channel accepts only the user that owns the ledger.`
> `caller TEXT NOT NULL, -- the peer user that the platform layer checked, or the owner of the request file`

Failure scenario: a worker runs as the same OS user that owns the ledger. It opens the command socket directly, or sends a state-changing command that is not one of the 3 client-side refusals. The peer-user check passes, and the request row records only that same user, not an authenticated operator role or fleet-attempt origin. A hostile worker can therefore submit an operator request such as cancellation or `horch rescue drop <ref>`; the latter can delete the only copy of unique work. The stated client check does not protect the server protocol from a raw request.

Smallest change: make fleetd authorize every request kind at the server. Bind live-channel requests to the peer process and its recorded attempt ancestry, reject operator-only kinds from a fleet attempt, and define an operator-authenticated fallback for request files. Add a Phase 5 test that sends a raw operator request from a worker sandbox.

### T43-3 - HIGH - Job result reuse prevents required fetch and push retries

Lines: 723-738, 1081, 1148-1149, 1256, 1524.

> `A job with the same kind and input key is not run 2 times: its result is used again.`
> `A push retries with a backoff, and 3 failures give 1 needs-you item.`

Failure scenario: a push of an unchanged feature tip fails once because the network is temporarily unavailable. The backoff ends and fleetd queues the same push inputs, but the unique key finds the failed job and reuses its result instead of running the second attempt. The feature never receives the 3 real push attempts promised by FM-43. A periodic fetch has the same problem unless a builder invents a changing key, so one transient fetch failure can leave the remote tip stale for ever and block integration.

Smallest change: define `input_key` for every job kind. Keep tip-based reuse for deterministic jobs, and include a retry ordinal or schedule window for retryable push and periodic fetch jobs. State which failed results are reusable and add tests for a transient push failure and 2 consecutive fetch periods.
