# The worker wrapper sends no request of class operator: its facts go by files of the attempt that fleetd ingests

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --lib execution && cargo test -p horch-core --test p6_authorize --test wrapper_files --test wrapper_ingest && cargo test -p horch-e2e --test p5_wrapper_facts && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p5_wrapper_facts.rs`

## Goal

The wrapper of a worker (a fleet process) reads no operator token and sends no request of class
operator. Each fact that it reports today by such a request (launched, launch failed, progress,
done, the session records of its attempt) goes by a file in the folder of its attempt, or by a
request of class worker, and fleetd ingests it as untrusted input. After this task, each request
of class operator comes only from a command that the operator runs.

## Context

Found by T-898 (opus-hardening-191), gap G2 of its step 6. After T-898, fleetd refuses each
request of class operator from a fleet process (`fleet_process(peer)` is Yes or Unknown). But
the wrapper itself sends operator kinds: `worker_launched`, `worker_launch_failed`,
`worker_progress`, `worker_done`, `spawn_adopt`, `spawn_gate_note` and `session_records`
(T-884 made `ExecutionStore::update` send `session_records`; T-472 made `worker_launched` and
`worker_launch_failed`). The wrapper of an idea-lead and of each push attempt is a fleet process,
so these are refused now (seen: `session_records` refused in the requests rows of the e2e runs).
The design says the other way from the start: single writer, RR-5: 'a job process and a worker
wrapper write files, and fleetd ingests them'. And the operator boundary says that a worker must
not read the operator token.

Gap G1 of the same note, for your reading only: a request file in `run/requests/` needs only the
token and has no peer fact; the sandbox denies the token read and the folder write.

Read first: `horch ledger show T-898`, `T-884`, `T-472`, `T-480`, `T-482`, `T-490`, `T-892`,
`T-550`; in `docs/specs/fleetd/design.md` the single writer and RR-5, the 3 kinds of file and
'untrusted worker files' (A24), the wrapper files (`heartbeat.json`, `exit.json`, `result.json`,
the question file), the classes of a request, and the operator boundary.

## Steps

1. List each request that a wrapper, `horch spawn --attempt`, or a worker command sends today,
   with its kind, class and caller (file and line). Send the list in 1 NOTE. For each: the target
   path by the design (a file of the attempt, a class worker request, or 'not needed after
   Phase 6: remove').
2. Write the red tests first in the new file `crates/horch-e2e/tests/p5_wrapper_facts.rs`, with a
   served test fleetd and a push attempt: (a) the session record of the attempt (session id,
   transcript path, check-ins) is in the ledger after the launch; (b) a launch that fails is on
   record with its reason; (c) the requests table has no row of class operator whose peer is a
   fleet process, refused or applied; (d) the wrapper process never opens the operator token
   (prove it by a token file that the test makes unreadable for the wrapper, or by the trace
   that the product has).
3. Move each fact of step 1 to its target path. A file of the attempt is written atomically
   (write a temporary name, then rename), fleetd ingests it in the pass, checks form, size, that
   the attempt belongs to the run and that the writer facts agree with the recorded wrapper, and
   a bad file gives no row and 1 needs-you item. Do not keep 2 paths for 1 fact: remove the old
   request kinds that have no sender left, with a Retired entry where the protocol needs one
   (`spawn_adopt` has no client since T-895).
4. `horch spawn --attempt` runs before the wrapper exists: say which process it is (a child of
   fleetd, so not a fleet process of an attempt) and keep its launch facts on a path that fleetd
   trusts by construction, not by the token.
5. Run g52 (`crates/horch-e2e/tests/fleet18_observability.rs`), the stop and lifecycle tests of
   `taskledger.rs` and `arc_26` if it is green on the base; name each test that this change
   makes green or red.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`. Do not print the operator token.
- 1 door: each state change goes through `transition()`. Effects run after the commit. No schema
  step; if you need one, send QUESTION with 2 options and end your turn.
- Do not add an exception to the authorize rule or to the arch rule, and do not make an assert
  weaker.
- Shared files (`pass.rs`, `attempt_files.rs`, `execution/store.rs`, `lifecycle.rs`): small
  functions, few shared lines, merge the run branch each 30 minutes and before finish.
- An e2e test serves a test fleetd (`h.serve_fleetd()`), makes its workspace with
  `h.fleet_workspace()`, puts the ledger and the fleet root outside the state folder, and sets
  `HORCH_TEST_DOCS_FIXTURE` to an empty local folder. Each test command has a time limit. Run
  only your own test files. Send `horch ledger progress` each 5 min.
- Windows code stays small. Name each Windows risk in the done summary.

## Done when

- The 4 tests of step 2 pass, and the Check passes.
- The done summary names: the list of step 1 with the path of each fact, each kind that you
  retired, the answer of step 4, the tests of step 5, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
