# Agent behavior after complexity-sweep failures

The local transcript scan found **56 confirmed failure notices, 62 observed finish-retry call records, and 31 affected sessions across 4 project roots**. In 13 sessions the first recorded retry call reached a sweep pass. Named callbacks with explicit inputs account for 8 of those 13 sessions. The next useful change is to check the full set of changed source and test functions with the same analyzer and configuration as finish before submitting the first commit.

## Scope and counting

- The scan enumerated 4,183 Claude Code and 2,084 Codex JSONL files. It indexed 38 candidate sessions and confirmed failures in 31 Claude Code sessions and 0 Codex sessions. The only Codex file matching the exact finish template in the snapshot contains the current research activity. The search covers the recorded format and accessible files on this machine.
- The unit is a delivered failure notice or a direct foreground failure result. The script deduplicates tagged deliveries by message ID within a session and excludes inbox echoes, quoted instructions, and unrelated tool failures. It groups by session and task number and uses the session working directory, with worktree suffix removed, for project counts. The first observed failure may follow activity outside the transcript.
- The retry count covers tool calls containing a finish command in the interval from a failure to the next definitive sweep result in that session. Calls can include queued, rejected, or resumed finish invocations. It does not equate to completed sweeps. A reported pass includes a sweep pass followed by a different finish failure. The script pairs timestamps only when both records exist.
- The scan counts 31 initial notices, 25 subsequent failure notices, 27 intervals ending in a documented sweep pass, and 4 intervals without a definitive paired notice. In 2 of the 4, later transcript status or agent text indicates the sweep passed. In another, the local reproduction passed before a handoff. These indirect observations do not enter the timing sample.
- Separately, 459 static-analyzer JSON files and 172 Clippy JSONL files exist in the local evidence tree. Their raw outputs contain 73 static-analyzer findings and 1,126 Clippy cognitive-complexity diagnostics, including findings outside changed functions, repeat runs, and possible duplicate Rust target diagnostics. Those figures do not enter the transcript event totals.

At revision `d01cb8aeb7de51814543274e03105eb725a414a4`, the finish step selects sources from the task branch diff, groups TypeScript, JavaScript, and Python by analyzer configuration, and uses Clippy for Rust. It filters findings by changed-function spans. The failure note displays at most 10 findings and points to the full analysis. Detached finish sends the task and step statuses through Horch. See `docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md`, `crates/horch-core/src/taskledger/finish/complexity.rs:78,343,428`, `crates/horch-core/src/taskledger/finish/mod.rs:714`, and `crates/horch/src/cmd/taskledger_finish.rs:153,178`.

## Retry cost

| Measure | Timed intervals | Total | Mean | Median |
| --- | ---: | ---: | ---: | ---: |
| Failure notice to next definitive sweep result, including edits, waits, and reruns | 52 | 23.81 h | 27.47 min | 75.48 s |
| Last observed retry call to that result, including preceding and subsequent finish steps in the invocation | 52 | 3.75 h | 4.33 min | 11.23 s |

The requested extrapolation is **62 calls × 259.54 s mean = 4.47 h** of retry-call elapsed time. At the median, the same calculation yields about 11.6 min. A few long intervals dominate the mean. An approximately 18 h failure-to-result interval included an overnight authority decision and additional work. The estimate measures wall-clock interval time and does not establish labor time or analyzer CPU time. Parallel intervals cannot be added to infer fleet elapsed time.

Visible diagnostic strings yield these lower bounds. The notice cap, display differences in older output, and unjoined raw findings make the denominator incomplete.

| Metric | Visible finding entries | Project roots |
| --- | ---: | ---: |
| `scopeDepthRatio` | 29 | 4 |
| `peakLiveVariables` | 7 | 3 |
| `maintainabilityIndex` | 4 | 1 |
| Rust cognitive complexity | 4 | 1 |
| `globalDataComplexity` | 3 | 1 |
| `essentialComplexity` | 2 | 2 |
| `scopeDepthWeightedLoc` | 1 | 1 |

## First recorded retry passes

| Pattern | Sessions | Session IDs |
| --- | ---: | --- |
| Name the callback or predicate at module scope and preserve its inputs. Verify browser or autoplayer output where relevant. | 8 | `7e097021-856b-4557-8598-8d7d554a295a`, `2d0b2e83-c8d4-4dd4-88fe-607592f30191`, `61436e49-0362-45ce-8e36-43dc733f2d6f`, `8838a64b-3d46-48f4-99f8-84626cbc7ad6`, `a5236dfa-ee5f-4a7e-bf4f-51a67704c011`, `a9d6bcea-a123-45fc-8f76-7380bb292b02`, `b04434d7-de3a-4f40-9ccd-3bf82e2b48a2`, `f8eb175b-65b6-486c-a945-91f4eea6eb16` |
| Re-run the matching analyzer over every flagged source and test file. Flatten test registration and extract named fixture or scenario operations. Both sessions reported zero findings locally before retry. | 2 | `654d7a1f-8c96-4322-8952-d9d35ef6b145`, `7f241ae7-ebf9-4414-9fff-ade8cba3f40a` |
| Extract named Rust test phases and keep assertions. The sweep passes on the recorded retry, with intervening workflow activity. | 1 | `2767e573-31c6-44e5-87e0-48df07e931a6` |
| Confirm a Python parser misclassification and clarify the comment that it misread. These are parser cases. | 2 | `72c5c9ee-e6d7-475a-a9fa-cd85fce119d7`, `b0edb8fa-7038-4856-8a06-dd5af33a9c18` |

## Bad responses and failed approaches

These are lower bounds from inspected edits, agent descriptions, and searches for suppression directives and threshold changes. A zero indicates that inspection did not confirm that response. The 6 requested response categories are distinct from the additional retry traps below.

| Response | Confirmed sessions | Session IDs and outcome |
| --- | ---: | --- |
| Add a lint suppression | 0 | No confirmed added `#[allow(clippy::cognitive_complexity)]`, `eslint-disable`, or `noqa` in a response. |
| Raise a configured threshold | 0 | Agents inspected configuration or reproduced its thresholds. No confirmed threshold raise. |
| Extract meaningless helpers | 0 | No confirmed extraction with no coherent responsibility. Naming a real predicate or preserving a test phase qualifies as substantive. |
| Move branches inside a lambda to conceal complexity | 0 | The observed direction was generally the reverse. |
| Delete a test to get through the sweep | 0 | One agent temporarily removed a behavior line to check that a regression test failed, then restored it. |
| Ask for a waiver | 1 | `43ba90dd-e1ae-4859-85ce-009f16a1073b`. The agent requested a waiver for findings initially attributed to the diff base. The agent later refactored and reproduced the sweep locally. |
| Pack values into tuples to lower the live-variable count before checking whether the reported line is the function definition | 1 | `804b126a-9ce5-4038-b1a8-9dce4fea9d99`. Two structural rewrites missed a misidentified call site. The third retry passed the sweep after a targeted edit. |
| Fix the displayed findings while leaving the full report for later | At least 3 | `d8453ab3-d7b8-4091-8fd5-6e18017c2e74`, `d7d3c7c0-8132-41ca-8ad6-1013a9acdc87`, `5baf3388-cac4-4304-b87d-270e0946d6c7`. The sessions recorded 6, 6, and 5 failure notices respectively. |
| Retry on zero analyzer errors while warnings or configurations differ | At least 3 | `2160cd38-b4f9-42fe-bb82-97c9d944fbfe`, `6e100faf-e635-4771-b23f-24b8a2e986c2`, `b876c48b-c507-4d26-b88f-8bf074b45ab0`. All required another complexity repair. |

## Code shapes before the failure

- **Embedded callbacks in browser orchestration.** A recorded edit preserves the old `shot` body with an inline browser callback nested inside the frame-capture function. The same renderer pattern recurs across copied episode sources. Another first-pass case names an autoplayer predicate that was previously nested. See `61436e49-0362-45ce-8e36-43dc733f2d6f`, `b04434d7-de3a-4f40-9ccd-3bf82e2b48a2`, and `7e097021-856b-4557-8598-8d7d554a295a`.
- **Whole test suites written inside a registration closure.** Pre-failure writes show `describe` wrapping long sets of inline tests or samples. Long source flows also bundled setup and scheduling in a single coordinator. Agents split scenario phases, flattened registration, and moved fixtures to named helpers. See `2160cd38-b4f9-42fe-bb82-97c9d944fbfe`, `654d7a1f-8c96-4322-8952-d9d35ef6b145`, and `7f241ae7-ebf9-4414-9fff-ade8cba3f40a`.
- **State accumulated across multiple responsibilities.** Follow-up edits show implicit module inputs in test helpers and multiple live values in render and selection loops. The working rewrites passed inputs, separated selection and rendering, or used a small lookup for uniform rules. See `2160cd38-b4f9-42fe-bb82-97c9d944fbfe`, `8c5bc457-4aad-4bf1-a92b-babf9af9c8e1`, and `d8453ab3-d7b8-4091-8fd5-6e18017c2e74`.
- **Long Rust scenarios combine setup, control flow, and verification.** Agents extracted test phases and assertion operations while retaining checks. The sample confirms these shapes in tests. It does not establish a separate count for growing match-arm lists or prove that single-edit size alone causes failures. See `2767e573-31c6-44e5-87e0-48df07e931a6` and `d97d49d4-aaff-4b7c-97b9-335002de8f69`.

## Ranked rules

The examples are schematic. **Verified** means an observed repair pattern or gate behavior supports the rule. It does not mean an experiment tested the effect of adding the instruction to an agent rules file. Counts refer to inspected sessions unless marked as failure notices or finding entries, and overlap across rules.

| Rank | Rule | Metric or lint | Evidence | Bad example | Good example | Target | Status |
| ---: | --- | --- | --- | --- | --- | --- | --- |
| 1 | Run the finish-equivalent analyzer on changed source and test files with the effective configuration and require zero blocking findings before the first finish call. | All gate metrics | 2 first-pass sessions and 3 warning or configuration retry sessions, 1 project | Check errors with CLI defaults. | Check the same files, configuration, and warnings as finish. | Project `AGENTS.md` | Verified |
| 2 | Use named module-level callbacks with explicit inputs for browser evaluation, event handlers, and nested predicates when they add a scope boundary. | `scopeDepthRatio` | 8 first-pass sessions, 2 projects | `function shot(k) { return page.evaluate(x => seek(x), k) }` | `function seekFrame(k) { seek(k) }` and `page.evaluate(seekFrame, k)`. | `.claude/rules/complexity-js.md`, `**/*.{ts,tsx,js,jsx,mjs,cjs}` | Verified |
| 3 | Register TypeScript tests at top level and give each test one scenario with named fixtures or phase operations when setup is shared. | `scopeDepthRatio`, `scopeDepthWeightedLoc`, `maintainabilityIndex` | At least 6 affected sessions, 1 project | One `describe` with inline cases and repeated nested fixtures. | Top-level `it` calls with a shared fixture builder. | `.claude/rules/complexity-tests.md`, `**/*.test.{ts,tsx}` | Verified |
| 4 | Read the full analyzer evidence and check every changed function before retrying a failed sweep. | All gate metrics | At least 3 repeat-failure sessions, 2 projects | Repair the 10 visible findings and retry immediately. | Inspect all findings, including warning entries, then rerun locally. | Project `AGENTS.md` | Verified |
| 5 | Extract named Rust test phases or repeated assertion operations while preserving every behavior check. | `clippy::cognitive_complexity` | 4 visible finding entries and at least 3 repaired sessions, 1 project | One test interleaves start, deadline polling, termination, and queue assertions. | Call `start_fetch`, `wait_for_termination`, and `assert_rescue_runs`. | `.claude/rules/complexity-rust.md`, `**/*.rs` | Verified |
| 6 | Confirm the reported line and diff ownership before refactoring a suspected parser false positive. | `peakLiveVariables`, changed-function filtering | 3 parser-related sessions, 1 project | Rewrite a function when the diagnostic points to a call or comment. | Reproduce the diagnostic on the exact line and verify unchanged output after the minimal correction. | Project `AGENTS.md` | Verified |
| 7 | Pass changing state and external dependencies explicitly to helpers that otherwise read several module globals. | `globalDataComplexity` | 3 visible entries in 1 project, including an observed passing input-parameter repair | `seedQueue()` reads client, repository, and seed data from globals. | `seedQueue(client, repository, seeds)`. | `.claude/rules/complexity-js.md`, `**/*.{ts,tsx,js,jsx,mjs,cjs}` | Verified |
| 8 | Use a lookup table for parallel cases that differ only in data and keep exceptional behavior in an explicit branch. | `essentialComplexity`, `scopeDepthRatio` | 1 traced refactor session with 6 failure notices, 1 project | Nested branches choose a gain based on cell type. | Index `gainByCellType[type]` and handle exceptions separately. | `.claude/rules/complexity-js.md`, `**/*.{ts,tsx,js,jsx,mjs,cjs}` | Not verified independently |
| 9 | Give every extracted helper a distinct operation and explicit inputs before using extraction to lower a score. | Cognitive complexity, `peakLiveVariables`, maintainability | 0 confirmed meaningless extractions, with 1 ineffective tuple workaround in 1 project | `part2(a, b, c, d)` continues half of a loop body. | `particlePosition(particle, frame)` returns a coordinate used by rendering. | Project `AGENTS.md` | Not verified as a preventative rule |
| 10 | Keep branching logic visible at its owning scope and forbid moving it into an inline lambda to conceal its score. | `scopeDepthRatio`, cognitive complexity | 0 concealment cases, with 8 sessions in 2 projects supporting named callbacks | Wrap a conditional chain in an immediately invoked arrow to lower its reported score. | Name the decision operation and pass its inputs. | `.claude/rules/complexity-js.md`, `**/*.{ts,tsx,js,jsx,mjs,cjs}` | Not verified for deliberate concealment |
| 11 | Preserve the behavior checks and forbid deleting or skipping a test to clear a complexity finding. | Test-file complexity and coverage | 0 confirmed deletions, with observed preserved assertions in 2 projects | Replace a long test with `it.skip`. | Extract fixtures and leave the assertion intact. | Project `AGENTS.md` | Not verified for deletion prevention |
| 12 | Keep complexity lint suppressions and threshold increases out of a complexity repair. | All gate metrics, including `clippy::cognitive_complexity` | 0 confirmed suppressions or raises | Add `#[allow(clippy::cognitive_complexity)]` to a failing test. | Split a named test phase and run the existing lint and test. | Project `AGENTS.md` | Not verified |
| 13 | Bring a reproducible analyzer or diff-base defect to the maintainer and forbid requesting a waiver for owned code. | Changed-function filtering and all gate metrics | 1 waiver request and 3 parser-related sessions in 2 projects | Ask to waive a finding based solely on the reported filename. | Confirm the diff base and source span and report a minimal reproduction. | Project `AGENTS.md` | Not verified as a preventative rule |

## Session rollup

The interval column lists one entry per failure notice, in order. Each numeric value is the elapsed time to the next definitive sweep result, rounded to seconds. “Open” means the summary could not pair a definitive result. The actions summarize inspected changes across the session. The IDs identify local sessions.

| Session ID | Task | Retry calls | Intervals in s | Observed response |
| --- | --- | ---: | --- | --- |
| `72c5c9ee-e6d7-475a-a9fa-cd85fce119d7` | T-408 | 1 | 4951 | Clarified a Python comment that the parser treated as code. |
| `7e097021-856b-4557-8598-8d7d554a295a` | T-46 | 1 | 60 | Named the autoplayer predicate. |
| `804b126a-9ce5-4038-b1a8-9dce4fea9d99` | T-5 | 3 | 30, 30, 38 | Split particle calculations, bundled live values, then fixed the reported call site. |
| `d8453ab3-d7b8-4091-8fd5-6e18017c2e74` | T-28 | 6 | 143, 75, 59, 192, 109, 17 | Separated page script, flattened selection, used a gain table, and named the seed callback. |
| `13135d64-f399-40ab-9d58-0fffaf21ab57` | T-4 | 2 | 152, 53 | Split rendering stages, then separated pixel scan and edge treatment. |
| `2d0b2e83-c8d4-4dd4-88fe-607592f30191` | T-14 | 1 | 30 | Reused named browser callbacks from the passing renderer. |
| `4e493e2f-7117-4050-9db3-84ad43a6f518` | T-13 | 2 | 358, 221 | Named browser-side operations, then checked all metrics locally and flattened remaining loops. |
| `61436e49-0362-45ce-8e36-43dc733f2d6f` | T-16 | 1 | 48 | Named browser frame callback. |
| `8838a64b-3d46-48f4-99f8-84626cbc7ad6` | T-39 | 1 | 31 | Named page event callbacks. |
| `a5236dfa-ee5f-4a7e-bf4f-51a67704c011` | T-31 | 1 | 30 | Named two browser event callbacks. |
| `a9d6bcea-a123-45fc-8f76-7380bb292b02` | T-15 | 1 | 29 | Named three browser event callbacks. |
| `b04434d7-de3a-4f40-9ccd-3bf82e2b48a2` | T-11 | 1 | 52 | Named browser frame callback and compared pixels. |
| `b0edb8fa-7038-4856-8a06-dd5af33a9c18` | T-138 | 1 | 34 | Clarified a comment reported as a Python function. |
| `f8eb175b-65b6-486c-a945-91f4eea6eb16` | T-41 | 1 | 20 | Named page event callbacks. |
| `2160cd38-b4f9-42fe-bb82-97c9d944fbfe` | T-10 | 3 | 347, 116, 66 | Flattened test setup, addressed warnings, then passed module inputs explicitly. |
| `3c095553-d971-436f-9cd4-e2317e6edbe3` | T-8 | 2 | 364, 77 | Flattened source and tests, then removed inline arrows from the remaining test helpers. |
| `5baf3388-cac4-4304-b87d-270e0946d6c7` | T-1 | 5 | 78, 73, 66, 76, open | Split source and test operations and replaced a remaining inline close callback with a bound method. |
| `654d7a1f-8c96-4322-8952-d9d35ef6b145` | T-6 | 1 | 367 | Checked the exact analyzer and flattened source and test fixtures. |
| `6e100faf-e635-4771-b23f-24b8a2e986c2` | T-5 | 2 | 505, 56 | Flattened callbacks and test bodies, then fixed warnings. |
| `7f241ae7-ebf9-4414-9fff-ade8cba3f40a` | T-11 | 1 | 845 | Reproduced the configured analyzer and split source and test phases. |
| `820ae6d7-9a6b-4837-92bb-6dd8c043764d` | T-9 | 1 | open | Reproduced the configured analyzer and flattened source and test operations before handoff. |
| `8c5bc457-4aad-4bf1-a92b-babf9af9c8e1` | T-7 | 2 | 137, 25 | Split manifest checks, then split budget and simplified a secret check. |
| `b876c48b-c507-4d26-b88f-8bf074b45ab0` | T-3 | 2 | 544, 149 | Reproduced errors, then fixed the remaining maintainability warning. |
| `d7d3c7c0-8132-41ca-8ad6-1013a9acdc87` | T-2 | 6 | 74, 50, 74, 51, 80, 20 | Reworked test mocks and fixtures, split assertions, flattened source and simplified the last reason expression. |
| `f494c11d-7f29-4998-b95c-eb397a5f915b` | T-4 | 1 | open | Reproduced the wrapper, flattened source and test registration, and reported later passage. |
| `1c9d13c0-36e1-44fa-989e-290fa4911e12` | T-6 | 1 | open | Simplified cover writing and duration selection. Later status indicated passage. |
| `2767e573-31c6-44e5-87e0-48df07e931a6` | T-124 | 1 | 2129 | Split three Rust scenario phases and retained assertions. |
| `43ba90dd-e1ae-4859-85ce-009f16a1073b` | T-221 | 5 | 64846, 5792 | Requested a waiver, reproduced the diff locally, refactored flagged files, then split the remaining Rust scenario. |
| `99f7528c-af58-4c6d-b7fb-a7bc8516bde5` | T-57 | 2 | 767 | Split a hung-fetch test into five named operations. |
| `acb33fcf-9ec4-45de-b879-e4f22bbf2ca4` | T-53 | 2 | 497 | Extracted the repeated ignored-note assertion. |
| `d97d49d4-aaff-4b7c-97b9-335002de8f69` | T-319 | 2 | 667 | Split the hung-fetch scenario into setup, termination, and rescue checks. |

## Local reproduction

Scripts are under `/Users/matthewscott/.local/state/horch/researcher-9/`. The index contains private tool records. Keep it local and out of Git. The summary emits session IDs, record numbers, diagnostic names, and timing. The review helper displays local agent text for manual classification.

```sh
cd /Users/matthewscott/.local/state/horch/researcher-9
python3 audit.py index.jsonl
python3 summarize.py index.jsonl
python3 review.py index.jsonl 61436e49
```
