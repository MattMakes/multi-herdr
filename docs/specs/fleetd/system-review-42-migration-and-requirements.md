# Systems review 42: migration plan and requirements

Scope: `docs/specs/fleetd/design.md` at `043cc279b99c951bdadf6adf5211df96eca6f156`.

## Verdict

NOT SOUND

Two phase gates require mechanisms that the migration plan adds only in later phases, so the sequence cannot be implemented as written. The design also leaves the immediate cutover of in-flight runs, the old watchdog shutdown, and several no-loss preservation rules without migration contracts or green gates.

## Answers

### 1. Can each phase pass its gate from prior work and its own work?

No. Phase 0 and Phase P are self-contained because expected failures remain red and PL-6 compares before and after results; Phases 2, 3, 5, 6 and 7 have enough stated work for their named gates (`design.md:1259-1260`, `1262-1263`, `1265-1267`). Phase 1 is not self-contained: it adds I1 before the Phase 2 lease schema, gates FM-13 before Phase 3 rescue, and gates FM-7 even though RR-5 becomes true only in Phase 5 (`design.md:647-660`, `1225`, `1261-1265`). Phase 4 is not self-contained because its FM-9 gate includes judgment-task judges, while Phase 6 adds judge tasks and the branch tables used by the Phase 4 landing state machine (`design.md:1221`, `1264`, `1266-1277`).

| Phase | Result | Gate evidence available at that point |
| --- | --- | --- |
| 0 | Yes | Expected-failure tests are the gate, so no later implementation is needed (`design.md:1259`). |
| P | Yes | PL-2 and PL-6 use Phase 0 baselines and Phase P parity (`design.md:1260`). |
| 1 | No | I1 needs Phase 2 leases; FM-13 needs Phase 3 rescue; FM-7 cites Phase 5's RR-5 (`design.md:648`, `1219`, `1225`, `1261-1265`). |
| 2 | Yes | Leases, capped waits, fallback, and quota waiting are all added here (`design.md:1262`). |
| 3 | Yes | Worktree ownership, rescue, reuse, reconciliation, and the FM-25 to FM-28 gate are added together (`design.md:1263`). |
| 4 | No | FM-9 includes the paired-judge path, but judge tasks arrive in Phase 6; the landing tables also arrive in Phase 6 (`design.md:1221`, `1264`, `1266`). |
| 5 | Yes | File reports, the command channel, read-only readers, and the FM-17/FM-24 gate are together (`design.md:1265`). |
| 6 | Yes | Ideas, plans, features, judges, questions, and their named FM gates are together (`design.md:1266`). |
| 7 | Yes | Docs sync, conformance, exec mode, and FM-18 to FM-20 are together (`design.md:1267`). |

### 2. Who writes the ledger in Phases 1 to 4?

fleetd writes from Phase 1, but existing agents and `horch` state-changing commands continue to write directly until Phase 5; the design itself says agents write the ledger today and moves commands to requests or files only in Phase 5 (`design.md:495-497`, `1261-1265`). Therefore the Summary's sole-writer statement is true only after Phase 5 (`design.md:15`). Phase 1's FM-7 gate and I1 to I5, Phase 2 lease transitions, Phase 3 worktree reservations, and Phase 4 verifier transitions all need a defined multi-writer compatibility rule before RR-5 is true, but the design is silent (`design.md:645-660`, `1219`, `1261-1265`).

### 3. When do the old watchdog and new fleetd paths go away?

The new fleetd path starts in Phase 1, and that phase removes only the lazy `ensure` start of the old watchdog (`design.md:1261`). The old watchdog currently performs checks and reaps, not only notifications (`design.md:99-102`, `1098`, `1191`), but no phase says to stop an already running watchdog, remove `horch ledger watch`, or transfer its per-run locks. Thus Phase 1 can have two control paths act on one task after an immediate update; the design is silent on the cutover.

### 4. What happens to a run in flight at each automatic release?

The design says every release is immediate and globally on, but gives only generic forward-only schema rules, backups, and newer-schema refusal (`design.md:1253-1257`). It does not define row backfills, old-process compatibility, or a per-phase in-flight test for Phases 1 to 7, so forward-only is stated for the schema version but not for each new table, column, constraint, and state. The phase-specific gaps are below.

| Release | In-flight rule in the design |
| --- | --- |
| Phase 0 | Tests only; no runtime cutover (`design.md:1259`). |
| Phase P | Behavior parity is explicit, so existing runs should see no semantic change (`design.md:1260`). |
| Phase 1 | Silent on stopping an old watchdog and adopting its live runs into fleetd (`design.md:1261`). |
| Phase 2 | Silent on backfilling a lease for an already held task and mapping existing waits into the new states (`design.md:1262`). |
| Phase 3 | The first orphan scan is report-only, but existing worktree rows have no stated state, owner, base, or rescue backfill (`design.md:1263`). |
| Phase 4 | Silent on old claim-based workers that lack `task.yaml`, `START.md`, and `result.json`, or a detached finish already running (`design.md:1264`). |
| Phase 5 | Silent on old workers that still invoke direct ledger commands and have no attempt outbox prepared for file reports (`design.md:1265`). |
| Phase 6 | Silent on assigning existing runs and tasks to required idea and feature rows during schema v9 migration (`design.md:1266`). |
| Phase 7 | Silent on whether already interactive workers continue unchanged when exec becomes the default (`design.md:1267`). |

### 5. Traceability from requirements and failure modes to phase gates

The tables below trace a requirement through the failure mode named by a green phase gate; `partial` means the gate proves only part of the requirement. Requirements with no complete green gate are FR-4, FR-6, FR-7, FR-12, RR-3, RR-6, RR-7, RR-8, RR-12, SR-1 to SR-4, PL-1, PL-3, PL-4, OR-1 to OR-6. Failure modes with no green gate are FM-11, FM-14, FM-15, FM-21, FM-22, FM-23, FM-33, FM-34 and FM-35 (`design.md:1259-1267`).

| Requirement | Green phase gate that proves it |
| --- | --- |
| FR-1 | Phase 6 through FM-8. |
| FR-2 | Phase 6 through FM-8, FM-16 and FM-31. |
| FR-3 | Phase 6 through FM-1 and FM-10. |
| FR-4 | Partial: Phase 4 FM-9 proves verification, but no gate proves the Phase 6 judge-plan rule or FM-33. |
| FR-5 | Phase 4 through FM-2. |
| FR-6 | Partial: Phase 6 FM-10 checks a missing profile, but no gate proves every required profile field and adapter refusal. |
| FR-7 | None; FM-15 is ungated. |
| FR-8 | Phase 7 through FM-18 and FM-19. |
| FR-9 | Phase 7 through FM-20. |
| FR-10 | Phase 6 through FM-1, FM-30 and FM-32. |
| FR-11 | Phase 3 through FM-25 and FM-26. |
| FR-12 | None; the Phase 6 gate does not name the direct-intake test. |
| RR-1 | Phase 1 through FM-4 and FM-13, then Phase 2 through FM-5. |
| RR-2 | Phase 2 through FM-3. |
| RR-3 | Partial: Phase 4 through FM-6 and FM-9; no gate proves judgment completion through FM-33. |
| RR-4 | Phase 1 through FM-7 and FM-13. |
| RR-5 | Phase 5 through FM-24, although Phase 1 FM-7 cites it too early. |
| RR-6 | None; FM-21 is ungated. |
| RR-7 | Partial: Phase 1 FM-12 and FM-13 test restart effects, but FM-11 is ungated. |
| RR-8 | None; FM-11 and FM-23 are ungated. |
| RR-9 | Phase 3 through FM-25 and FM-27. |
| RR-10 | Phase 3 through FM-26 and FM-28. |
| RR-11 | Phase 2 through FM-5 and its explicit quota assertion. |
| RR-12 | None; FM-35 is ungated. |
| SR-1 | Partial: Phase 5 FM-17 proves one shared write; no phase gate names full sandbox conformance. |
| SR-2 | None. |
| SR-3 | None. |
| SR-4 | None; FM-9 does not test that acceptance runs with worker rights. |
| PL-1 | Partial: Phase 1 FM-12 tests one all-OS behavior, not full fleetd operation on all 3 OSes. |
| PL-2 | Phase P names PL-2 directly. |
| PL-3 | Partial: Phase P PL-6 preserves existing unsupported cases, but no gate names every explicit unsupported return. |
| PL-4 | None; Phase 0 makes its sleep test red, but no later gate names it green. |
| PL-5 | Phase 1 through FM-12. |
| PL-6 | Phase P names PL-6 directly. |
| OR-1 | Partial: Phase 3 FM-28 uses status, but no gate proves the complete idea/task/lease/worker view. |
| OR-2 | None. |
| OR-3 | Partial: Phase 6 FM-16 and FM-29 cover some escalations; FM-35 is ungated. |
| OR-4 | None. |
| OR-5 | None. |
| OR-6 | None; Phase 0 makes restart red and FM-34 is ungated. |

| Failure mode | Green phase gate |
| --- | --- |
| FM-1 | Phase 6 |
| FM-2 | Phase 4 |
| FM-3 | Phase 2 |
| FM-4 | Phase 1 |
| FM-5 | Phase 2 |
| FM-6 | Phase 4 |
| FM-7 | Phase 1, but it prematurely cites RR-5 |
| FM-8 | Phase 6 |
| FM-9 | Phase 4, but its judgment path depends on Phase 6 |
| FM-10 | Phase 6 |
| FM-11 | None |
| FM-12 | Phase 1 |
| FM-13 | Phase 1, but its rescue path depends on Phase 3 |
| FM-14 | None |
| FM-15 | None |
| FM-16 | Phase 6 |
| FM-17 | Phase 5 |
| FM-18 | Phase 7 |
| FM-19 | Phase 7 |
| FM-20 | Phase 7 |
| FM-21 | None |
| FM-22 | None |
| FM-23 | None |
| FM-24 | Phase 5 |
| FM-25 | Phase 3 |
| FM-26 | Phase 3 |
| FM-27 | Phase 3 |
| FM-28 | Phase 3 |
| FM-29 | Phase 6 |
| FM-30 | Phase 6 |
| FM-31 | Phase 6 |
| FM-32 | Phase 6 |
| FM-33 | None |
| FM-34 | None |
| FM-35 | None |

### 6. Does the migration keep every Preservation analysis row?

No. The design's final architecture names many rows, but the migration plan does not assign every row to a phase or gate before old paths change. The grouped ledger below covers every row in the Preservation analysis and identifies the rows that the migration can lose (`design.md:1079-1203`).

| Preservation rows | Phase or requirement that keeps them | Migration result |
| --- | --- | --- |
| 04 F9, F7, F5, F11, L6 | Phase 4 explicitly carries them into landing (`design.md:905-912`, `1264`). | Kept. |
| 04 S7, S8 | Phase 1 transactional reconcile and exact-PID action; PL-2/PL-6 preserve the process identity implementation (`design.md:1098`, `1101`, `1260-1261`). | Kept, but S7 has no direct green gate. |
| 04 S10 | OR-6 and the rollout lead retain binary restart (`design.md:1102`, `1255`). | Kept, but FM-34 is ungated. |
| 04 W6, L5, L9, W8, W9, L8 | Final invariants and rescue cover parts, but no phase gate preserves stash refusal, parked unlanded work, forward-only archive refs, triage refusal, or attempt-floor reset before Phase 3/4 cleanup changes (`design.md:1089`, `1093-1100`, `1263-1264`). | Can be lost. |
| 04 M5 | No phase or requirement keeps private ownership and modes for the new outboxes (`design.md:1103`, `1265`). | Can be lost. |
| 05 E-10 and 06 S1; 06 S2, S3, S9 | The adapter narrative names key scrubbing and sandboxing, but no migration gate carries all child paths, the herdr-resume warning, Google-key scrubbing, or sandbox-host refusal (`design.md:1104-1107`). | Can be lost during adapter and pane rewrites. |
| 06 S4, S5, S6 | Phase 7 says adapters carry over and names private homes; Phase 4 adds hooks, but hook pre-trust remains an open test (`design.md:1108-1110`, `1299`). | Partial; can be lost before Phase 7 or when hook trust changes. |
| 06 S7 | The rollout lead keeps the release cooldown (`design.md:1111`, `1255`). | Kept. |
| 06 S8, S10 | No phase keeps pane auto-update off or the narrow, locked, atomic trust-store writes (`design.md:1112-1113`). | Can be lost. |
| 06 S11 | Phase P moves the socket sites and Phase 5 removes the worker-channel need while retaining a Windows command transport (`design.md:1114`, `1260`, `1265`). | Kept. |
| 06 S12 | OR-5 and Phase 1 health guards retain unknown probe results (`design.md:1115`, `1274`). | Kept, but ungated. |
| 06 S13 | OR-4, the carried adapters, and Phase 1 pacing retain CLI-owned quota reads (`design.md:1116`, `1274`). | Kept, but ungated. |
| 06 S14 | Phase P and Phase 1 provide the 3 OS platform and hang paths (`design.md:1117`, `1260-1261`). | Kept. |
| 06 S15 | The adapter section explicitly carries OpenCode, pi and Prime (`design.md:1118`, `723`). | Kept, but no gate names all 3. |
| Mail contract; message spool | Phase 6 retires the old orchestrator, but no phase preserves mail delivery, sender tags, retries, the 600-character spool, or 7-day retention (`design.md:1124`, `1132`, `1266`). | Can be lost. |
| Unlanded report | No phase keeps `UNLANDED`, `UNREGISTERED`, `unfinished`, `reopen`, and `dismiss` as operator-visible decisions (`design.md:1125`, `1093-1094`). | Can be lost. |
| `horch self-update` | OR-6 and the rollout lead retain it (`design.md:1126`, `1255`). | Kept, but ungated. |
| Release assets | No phase or requirement preserves the archive names, checksums, and 5 targets (`design.md:1127`). | Can be lost. |
| Ticket and MAP formats | Phase 6 keeps Markdown tickets and intentionally replaces MAP with `plan.yaml` (`design.md:1128-1129`, `1266`). | Kept or deliberately replaced. |
| Project facts | The workspace protocol sends facts to every sub-orchestrator (`design.md:1130`, `352`). | Kept, but the 100-item/2048-byte limits are not. |
| Evidence logs | Phase 4 keeps today's review scripts and profiles unchanged (`design.md:1131`, `1264`). | Kept by intent, not by a named gate. |
| Exit codes | No phase or requirement preserves exit codes 0, 1, 2 and 3 while commands change (`design.md:1133`). | Can be lost. |
| V-10, V-11, V-14, 04 F11 | Phase 4 carries the current finish, review, lock, check, and network behaviors (`design.md:1139-1142`, `1264`). | Kept. |
| V-23 and L-4 | OR-6 and every schema phase keep backups and newer-schema refusal, but the 30 s busy timeout is not assigned (`design.md:1143`, `1278`). | Partial; busy timeout can be lost. |
| V-25 | Facts carry over, but their 100-item and 2048-byte bounds have no phase or gate (`design.md:1144`). | Can be lost. |
| V-26 | Phase 1 adds pacing, but no phase preserves the current 0.85, 0.98, 1.25x, 0.50 and 15 min routing values (`design.md:1145`, `1274`). | Can be lost. |
| V-17 | No phase preserves the inline limit and spool retention (`design.md:1146`). | Can be lost. |
| Update cooldown | The rollout lead explicitly keeps it (`design.md:1147`, `1255`). | Kept. |
| Harness command/build and launch flow; context readers; trust writes; herdr calls | Phase P preserves behavior and Phase 1/7 replace parts, but no gate names env scrub coverage, context-reader wiring, narrow trust writes, or herdr timeout/process-group behavior (`design.md:1153-1154`, `1159`, `1161-1162`). | Partial; can be lost. |
| OpenCode; pi and Prime; private CODEX_HOME; quota probes; update check/cooldown; machine probe | Adapter carryover, OR-4/OR-5, Phase 1, Phase 7 and the rollout lead name these paths (`design.md:1155-1160`, `1163`). | Kept by intent, but no gate proves every registration. |
| Claude subscription; Codex ChatGPT; Google and OpenCode provider logins; Prime login; local pi | The architecture names each source without values and intends the adapters to carry them over (`design.md:1169-1176`). | Partial; no phase gate proves every source and rotation path after rewrite. |
| Git SSH agent; `gh` login; unauthenticated self-update; unauthenticated npm | Landing, pull-request, self-update and cooldown sections retain their consumers (`design.md:1177-1180`). | Kept by intent, but no phase gate proves the credential-source rule. |
| Cleanup loss; overwritten refs; unlanded work; permanent escalations; API-key leaks; Google-key leaks; planted local instructions; test/scratch shared-ledger writes | These failure-analysis rows map to W6/W8/W9/L5/L9/L8, E-1/E-2, E-10/S1/S2/S3, and M5, which have no complete migration gate (`design.md:1186`, `1188-1189`, `1192`, `1198-1201`). | Can be lost. |
| Cancelled/rebased landing; stalled git; reap transaction; overlapping paths; long suites; schema safety; sandbox refusal; exact-host stop | Phase 1, Phase 4, OR-6, PL-2/PL-6 and adapter conformance carry these rows (`design.md:1187`, `1190-1197`, `1202-1203`). | Kept by intent; several remain ungated as listed in Answer 5. |

### 7. What does Phase P before stall relief cost?

Phase P moves 178 production sites before Phases 1 to 4 stop daily stalls and lost worktrees (`design.md:23`, `294`, `1251`, `1280`). This LOW decision cost is a longer lead time to operational relief and a large behavior-parity refactor that can consume debugging capacity before users receive the control-loop benefits. This order is decision D16 and is not a proposal to change the order (`design.md:45`).

### 8. Are the listed numbers starting values or fixed contracts, and where are they configured?

Only the resolver size is explicitly called a starting value; several other values are defaults or preserved contracts, but the design is silent on whether they may change and on some configuration locations. `fleet.toml` is said to hold global timers and health floors, profiles hold worker liveness and retry values, and Phase 7 changes `context-windows.md` (`design.md:320`, `695-711`, `1267`). The complete classification follows.

| Number | Classification in the design | Configuration location |
| --- | --- | --- |
| 5 s reconcile | Called a default timer at `design.md:1209`, not explicitly a starting value or fixed contract. | Presumably `fleet.toml` timers from `design.md:320`; no key is named. |
| 30 s daemon check/stale heartbeat | Written as a PL-5 shall and an OS service rule, so it is a contract (`design.md:180`, `288`). | Platform service definitions and `horch fleetd check`; no config key is named. |
| 30 s wrapper heartbeat age | Used as a lease-renewal condition (`design.md:967`) but not classified. | Silent. |
| 30 s retry backoff | A profile example value in `retry.backoff_s` (`design.md:705-707`). | Worker profile. |
| 20 s stop grace | A Linux platform behavior, not marked starting or configurable (`design.md:290`, `977`). | Silent; the Linux platform module is implied. |
| 1 h interactive wait | A default profile value and an operator-settled cap, not called a starting value (`design.md:704`, `1295`). | Worker profile `liveness.max_wait_s`. |
| 1 h land lock | A preserved current contract (`design.md:1139`). | Current `finish/land.rs`; no fleetd config is named. |
| 256000 context | A default and quality rule (`design.md:62`, `141`, `716`). | Profile `compact_window`, native CLI setting, and Phase 7 `context-windows.md`. |
| 20 GB worktree budget | Called a default, not a starting value or fixed contract (`design.md:865`). | Key `worktree_disk_budget` is named; its file is not, though `fleet.toml` is the likely global location. |
| 100 lines or 2 files | Explicit starting values to tune from data (`design.md:935`, `1305`). | Silent; no splitter config key or file is named. |
| 3 attempts | A schema/profile default, not a starting value or fixed contract (`design.md:546`, `706`). | Worker profile `retry.max_attempts`; schema default also exists. |
| 2 fix rounds | A fixed judgment workflow rule from D4, with no configurability stated (`design.md:282`). | Silent. |

## Findings

### S42-1 - CRITICAL - Phase 1 and Phase 4 require later-phase mechanisms

Lines: `design.md:647-660`, `1221`, `1225`, `1261-1266`.

> Add the fleetd concerns to the platform interface: service install and restart, hang detection, sleep and wake, file watching, and the transport of the fleetd command channel.
> *Gate:* FM-4, FM-7, FM-12, FM-13 green.

Failure scenario: Phase 1 starts with a running task but has no Phase 2 lease row for I1 and no Phase 3 rescue path for FM-13. A herdr restart leaves dirty work that the Phase 1 gate requires the system to rescue, so the exact gate cannot pass; a weakened gate can lose the work. In Phase 4, a judgment task reports success, but the paired judge machinery does not exist until Phase 6, so FM-9 either cannot pass or accepts an unjudged result.

Smallest change: move the required lease, rescue, and judge slices before their first gates, or narrow those gates to behavior available at that phase and add later green gates for the deferred paths.

### S42-2 - HIGH - Immediate releases have no in-flight data and process migration contract

Lines: `design.md:1253-1267`, `1278`.

> A phase is released when the tests of its gate pass, and it is on for every user at once.

Failure scenario: Phase 2 restarts fleetd while an older worker holds a task. The migrated row has no specified lease, so I1 can stop and requeue the live worker; Phase 4 can then start a result-gated attempt while an old detached finish still lands the same task. A builder must guess backfills and compatibility, and a wrong guess duplicates work or loses uncommitted state.

Smallest change: for every phase, specify old-row backfills, old-process handling, compatibility of commands and files, and one upgrade-in-the-middle test that starts on the prior phase and finishes on the new phase. Keep rollout immediate, as D9 requires.

### S42-3 - HIGH - Phase 1 does not retire an already running old watchdog

Lines: `design.md:99-102`, `1098`, `1191`, `1261`.

> Remove the lazy `ensure` start.

Failure scenario: an old per-run watchdog is alive when self-update installs Phase 1 and starts fleetd. The old watchdog reaps and requeues a task while fleetd applies its own liveness decision, so a new attempt can start while the earlier attempt still edits the task worktree. The operator sees duplicate attempts or conflicting changes.

Smallest change: make Phase 1 atomically enumerate and stop every recorded watchdog by exact PID and start time, retire its command and lock, then start fleetd; add a cutover test with a live worker.

### S42-4 - HIGH - The phase gates do not prove core requirements and 9 failure modes

Lines: `design.md:127-192`, `1205-1247`, `1253-1267`.

> The tests are the only gate.

Failure scenario: all named phase gates pass while RR-8 and FM-23 have no green gate. A crash during a result-file write or ledger ingestion is released without the atomic-write test, and the next pass can reject a partial result, duplicate an action, or lose task history. The same omission applies to secret handling, protected-branch controls, acceptance sandbox rights, and self-update compatibility.

Smallest change: add a traceability matrix to the migration plan and require every requirement verification plus every FM test to become green in exactly one phase. Keep expected failures until that named phase.

### S42-5 - HIGH - Destructive migration starts before no-loss preservation rows have owners

Lines: `design.md:1089-1100`, `1186-1192`, `1263-1264`.

> Archive and trash refs only move forward, and each ref write is read back before a folder is removed.
> *Gate:* FM-25 to FM-28 green; a worker killed with dirty and untracked files loses nothing.

Failure scenario: Phase 3 adopts an existing unlanded worktree, then a builder ports rescue without the current forward-only ref and read-back rules. A second rescue overwrites the first ref, cleanup removes the tree, and the unlanded decision record is absent, so the operator loses the earlier paid-for work. The design requires the builder to guess how W6, L5, L9, W8, W9 and L8 survive until the later branch model.

Smallest change: assign W6, L5, L9, W8, W9 and L8 to Phase 3 requirements and gate tests before any cleanup acts; keep report-only mode until each test passes.

### S42-6 - MEDIUM - Numeric contracts and configuration locations are incomplete

Lines: `design.md:87`, `290`, `704-710`, `865`, `935`, `1209`, `1305`.

> Starting values, to tune from data. A resolver task covers at most 100 changed lines or 2 files.

Failure scenario: one builder hard-codes the 5 s reconcile, 20 s stop grace, and 2 judge fix rounds while another exposes them in `fleet.toml`. Operators then tune one deployment but not another, and an upgrade silently changes stall or escalation behavior.

Smallest change: label each listed value as fixed, default, or starting, and name its config key and owning file or state that it is intentionally not configurable.

### S42-7 - LOW - Decision D16 delays relief behind a 178-site parity move

Lines: `design.md:23`, `45`, `294`, `1251`, `1280`.

> Then move each of the 178 production sites of `preservation-06-wiring-and-platform.md`, section 6, behind the interface.

Failure scenario: a parity regression in one of the 178 moved sites consumes the migration cycle, so users continue to experience daily stalls and orphaned worktrees while no Phase 1 to Phase 4 remedy can ship. This is a LOW decision finding because D16 fixes the order.

Smallest change: record the expected schedule and regression budget for Phase P so the operator can see the accepted delay; do not change the D16 order.

## Checked and sound

- Phase P now has a valid expected-failure parity gate: listed red tests remain red, and previously passing tests remain passing on all 3 OSes (`design.md:181`, `1259-1260`).
- Phase 3 performs one report-only orphan scan before it acts, which is the correct first step for unknown existing worktrees (`design.md:1263`).
- Phase 5 correctly sequences the final single-writer cutover after file-based worker reporting and a durable request fallback are available (`design.md:1265`).
- Each schema-changing phase explicitly retains a pre-migration backup and newer-schema refusal (`design.md:1257`, `1278`).
- D16's Phase P scope includes sites that later phases delete, so the platform rule has no silent migration exceptions (`design.md:1260`).
- The resolver size is correctly identified as a starting value and remains an explicit open test rather than a false fixed contract (`design.md:935`, `1305`).
