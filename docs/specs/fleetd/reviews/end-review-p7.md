# End review of phase 7

## Summary

- Critical: 1.
- Important: 3.
- Minor: 0.
- The phase gate is not verified: profile health can fail open, and the release evidence does not exercise real agent CLIs.

## Findings

### R6-F1 — Critical — conformance health can fail open for dispatch

**Claim: verified.** FR-8, FM-19 and the SR-8 Operator boundary require dispatch to use current conformance health. The pass dispatches at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1313` and does not process ended phase-7 jobs until `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1325`. A profile can therefore receive 1 task in the pass that first observes its failed conformance result.

The failure persists for a missing or malformed report. `act_on_conformance` writes no health mark for `Ok(None)` or `Err` at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4250`, but it records `CONFORMANCE_ACTED` for both at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4260`. `conformance_report` returns `None` when the result has no path and returns an error for unreadable or invalid JSON at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4434`. The scheduler has no immediate retry after that conformance row at `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs:135`.

Concrete case: a changed Codex binary writes no usable `conformance.json`. A ready Codex task binds before the result is applied, and later passes continue to treat the profile as healthy. If its sandbox boundary changed, the worker can receive Operator rights. The smallest fix is to process conformance before dispatch, hold a changed profile until a current report passes, and mark an invalid or missing report unhealthy instead of recording it as acted without a mark.

### R6-F2 — Important — the request-write boundary has an invalid precondition

**Claim: verified.** SR-8 requires the suite to prove that each protected Operator act works outside the sandbox and is refused inside it. `operator_reach` opens the token and connects to the channel, but it only checks `operator.requests.is_dir()` for `WriteRequests` at `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs:357`. It never proves that the runner can write the folder.

Concrete case: host permissions make `run/requests` unwritable while a rendered sandbox accidentally permits the path. The fake agent's write fails, and the check reports a sandbox refusal. An Operator later repairs the folder permissions, and the healthy worker can write Operator requests. The smallest fix is an outside-sandbox exclusive create and removal of a unique harmless probe file before launch, plus a test with an existing but unwritable request folder.

### R6-F3 — Important — Antigravity interactive attempts have no Stop gate

**Claim: verified.** FM-6 requires every push attempt to run `horch result --check` before it can stop. The launch tests define and assert Stop hooks only for Claude and Codex at `crates/horch-core/tests/harness_defaults.rs:761`. No Antigravity Stop mechanism is implemented. The current SR-8 validation refuses Antigravity worker profiles because no Operator boundary exists at `crates/horch-core/src/harness/antigravity.rs:269`, so the defect is dormant under current validation rather than safe to release as supported behavior.

Concrete case: if the Operator enables an Antigravity interactive profile after its sandbox work, the CLI can return to its prompt without a result or question, and fleet state waits for supervision instead of the Stop gate. The smallest fix is to keep interactive Antigravity explicitly unsupported until a tested Stop mechanism exists, or implement that mechanism before accepting its profile.

### R6-F4 — Important — the release evidence uses fake CLIs for real-CLI rules

**Claim: verified.** SR-1, SR-8, FM-18 and FM-19 depend on behavior of installed agent binaries. The Operator-boundary test states that it launches fake agents and never a real agent CLI at `crates/horch-core/tests/p7_operator_boundary.rs:1`. The exec suite also identifies its commands as fakes at `crates/horch-core/tests/p7_wrapper_exec.rs:30`. These tests prove adapter argv, stream parsing and rendered sandbox mechanics, but not that real Claude and Codex honor the prompt, hooks, Stop gate, private home, exact MCP set, or sandbox boundary.

Concrete case: a real CLI version changes a flag, hook format or child-process behavior while the fake continues to accept the adapter contract. The suite passes and marks the profile healthy although the protected behavior differs. The smallest fix is a release gate that runs the production conformance suite once with each supported real CLI in an isolated fleet root and retains its `conformance.json`. This review did not run that gate because the ticket forbids tests and does not authorize live agent execution.

## Watchlist lines covered

| Line / item | Result |
|---|---|
| 11 / T-357 Antigravity Stop hook | **Verified: R6-F3.** `crates/horch-core/tests/harness_defaults.rs:761` covers only Claude and Codex Stop hooks; Antigravity remains refused by `crates/horch-core/src/harness/antigravity.rs:269`. |
| 15 / T-485 Codex `network_proxy` boundary | **Not verified: R6-F4.** The fake check proves fail-closed behavior when the feature is absent at `crates/horch-core/tests/p7_operator_boundary.rs:416`; no real Codex sandbox probe is recorded. |
| 24 / T-488 and T-575 oracle diff | **Verified, checked with no fault.** Commit `213d39e0` changes the 18 launch oracles only for sandbox and Operator-boundary settings. Its routing changes only remove the separately approved idea-lead fallback, now declared at `teammates/idea-lead.md:9`; no unrelated teammate entry changes. |
| 39 / T-600 docs source without exec adapter | **Verified, checked with no fault.** `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs:17` explicitly gives such a source docs sync without conformance, matching MAP Q5. The project selection for a drift idea remains a design decision for the Operator. |
| 41 / T-881 codex-terra exec mode | **Verified, checked with no fault.** The wrapper exec test proves an exec attempt reaches a result and writes `exit.json` at `crates/horch-core/tests/p7_wrapper_exec.rs:226`. The staying-process observability fixture explicitly changes codex-terra to interactive at `crates/horch-e2e/tests/fleet18_observability.rs:404`; it no longer mistakes an exec profile for a persistent pane. |
| 44 / T-891 real CLIs become healthy | **Not verified: R6-F4.** `crates/horch-core/tests/p7_operator_boundary.rs:8` says the test never launches a real agent CLI. |
| 46 / T-891 Antigravity remains unhealthy | **Verified, checked with no fault.** `crates/horch-core/src/harness/antigravity.rs:269` returns `NotSupported` on every OS and explains the missing boundary. Release support still needs the Operator decision below. |
| 53 / T-891 Codex MCP rendering | **Verified, checked with no fault.** Validation excludes control characters and does not print environment values at `crates/horch-core/src/harness/codex.rs:1903`; rendering clears the table before exact profile entries at `crates/horch-core/src/harness/codex.rs:1986`; tests cover quoting, exact names and secret-free errors at `crates/horch-core/src/harness/codex.rs:4100`. |
| 65 / T-891 one real-Claude and real-Codex run | **Not verified: R6-F4.** No retained real-CLI result is present; fake-agent evidence cannot close this action. |
| 66 / T-561 and T-890 default fleet layout | **Verified, checked with no fault.** The default fleet root is `fleet` beside the ledger at `crates/horch-core/src/runtime/paths.rs:39`, not inside the worker-writable state root. The e2e layout is intentionally different. |
| 68 / T-561 permanently unhealthy profile | **Verified: R6-F3.** Dispatch intentionally has no unsafe fallback, and failed conformance creates drift work. A plan that names Antigravity still cannot run, so the Operator must exclude that profile from release or complete its missing rules. |
| 70 / T-898 real sandbox approval/write/token probe | **Not verified: R6-F4.** T-898 is an unlanded branch outside this review. Current fake boundary tests do not prove the real-CLI behavior requested by this line. |
| 71 / T-895 and T-603 Antigravity exec tests | **Verified, checked with no new fault.** `crates/horch-core/tests/p7_wrapper_exec.rs:30` labels the commands as fakes, so the tests prove only the adapter. R6-F4 carries the remaining real-binary gap. |
| 74 / T-898 orphan probe | **Not verified: R6-F4.** The current phase-7 boundary test has no orphan probe, and the named T-898 branch is outside scope. |
| 75 / T-898 request proof and permissions | **Verified: R6-F2.** The conformance preflight checks only that the request path is a directory at `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs:374`; it does not prove effective outside-sandbox write access. |

## Tests that do not prove their rule

- **SR-8 Operator boundary — not verified for real CLIs.** `crates/horch-core/tests/p7_operator_boundary.rs:1` launches fake agents. It proves the rendered policy around its fake child, not real CLI child-process or orphan behavior.
- **SR-8 request write — not verified.** `operator_boundary_fails_when_the_rules_allow_a_request_write` at `crates/horch-core/tests/p7_operator_boundary.rs:392` starts with a writable fixture. There is no test that distinguishes a sandbox denial from an unwritable host folder.
- **FM-19 health application — not verified for ordering and corrupt output.** Existing successful and failed-report cases do not prove that health changes before dispatch, nor that missing or malformed `conformance.json` fails closed. R6-F1 identifies both paths.
- **FR-6 and FM-18 exec behavior — not verified for installed binaries.** `crates/horch-core/tests/p7_wrapper_exec.rs:30` uses fake CLIs. It proves command and stream contracts only.
- **FM-6 Stop gate — not verified for Antigravity.** `crates/horch-core/tests/harness_defaults.rs:761` has no Antigravity Stop-hook case.

## Operator decisions

1. **Antigravity release support — decision required.** The reviewed implementation correctly refuses its profiles, but private HOME, the Operator boundary and a Stop mechanism are absent. Recommendation: list Antigravity as unsupported at release and do not accept its profiles until all 3 rules pass.
2. **Real-CLI release gate — decision required.** Recommendation: require one isolated production conformance run for each supported Claude and Codex version before release, and retain the report as release evidence. Fake-agent tests remain the deterministic CI layer.
3. **Docs-only sources — decision required for design wording.** The implementation gives herdr docs sync without conformance because it has no exec adapter. Recommendation: keep this behavior and state explicitly that docs-only sources do not create worker-profile health.
4. **Drift-idea project selection — decision required for design wording.** The implementation selects the first repository by path. Recommendation: preserve the deterministic rule and add it to the design before release.
