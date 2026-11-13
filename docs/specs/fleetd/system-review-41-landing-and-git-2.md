# Systems review 41, round 2: landing, git and no-loss rules

Scope: `docs/specs/fleetd/design.md` at `c8bf35ac2083746a95414f24cc3679bf2a16f1d6`, compared with `62337c29df6d5a2ce42e633a5ebd217160d5c6b1`.

## Verdict

FAIL

0 HIGH findings from the first review are still open. This review adds 1 CRITICAL and 2 HIGH findings.

## Answers

- S41-1 (CRITICAL) closed: Phase 4 now adds `features`, `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips`, and its gate needs the landing state machine and both gates (`design.md:1549`, `design.md:1565`, `design.md:1610`).
- S41-2 (CRITICAL) closed: a new idea moves a ready feature back to open, and a merge with an unfinished idea moves that idea to a new feature with its owner and rescued task branches (`design.md:1073`, `design.md:1077-1079`).
- S41-3 (HIGH) closed: durable `bundles` and `bundle_members` rows hold the base, candidate, parent and members, and restart continues each bundle from those rows (`design.md:686-704`, `design.md:1100`).
- S41-4 (HIGH) closed: each branch row records local and remote observed tips, and every ref move uses compare-and-set; a mismatch moves no ref, so the original overwrite and loss scenario stops (`design.md:670-683`, `design.md:829`).
- S41-5 (HIGH) closed: merge or abandonment of a lower feature clears the stack edge, rescues and rebases the upper feature, reruns gates, and retargets its pull request (`design.md:1147`).
- S41-6 (HIGH) closed: Phase 4 names one executable test for each invariant I8 to I12, and the gate table assigns all 5 tests to the first phase that uses them (`design.md:1549`, `design.md:1570-1584`).
- S41-7 (HIGH) closed: rescue refs now have explicit `unique` and `superseded` classes, fleetd ownership, deletion rules, byte reporting and a unique-work budget (`design.md:1127-1132`).
- S41-8 (MEDIUM) closed: feature rows now record push and pull request outcomes, failed pushes use bounded retries and escalation, closed pull requests create a needs-you item, and `remote = none` defines local-only integration (`design.md:577-590`, `design.md:1081`).

## New findings

### T41-1 - CRITICAL - A compare-and-set mismatch strands a passed bundle

Lines: `design.md:829`, `design.md:1092`, `design.md:1100-1102`.

> When the tip is not the expected one, fleetd moves nothing, records the new tip, makes the gate result stale and decides again on the next pass.
> When the tip of a feature branch moves while a bundle of that feature is in `building` or `gating`, the bundle goes to `dropped`.

Failure scenario: A bundle passes its fast gate against feature tip P, and the ledger records the bundle as `passed` before the ref-move effect. An operator or another allowed git actor moves the feature ref from P to Y before fleetd runs `update-ref`. The compare-and-set correctly refuses to overwrite Y. The gate is now stale, but only a bundle in `building` or `gating` can move to `dropped`. The bundle is already `passed`. Its task branches remain `in_bundle`, whose exits require a successful feature move, a dropped or failed bundle, or a build conflict. None occurs. The branches never return to `ready`, and their ideas cannot reach `integrating`.

Smallest change: Add an observed compare-and-set failure transition. It moves a `passed` bundle to `dropped` or `stale`, returns each `in_bundle` member to `ready`, clears the intended ref move, and rebuilds and regates from the newly observed feature tip.

### T41-2 - HIGH - Bundle candidate refs have no terminal cleanup rule

Lines: `design.md:686-704`, `design.md:1100`, `design.md:1127-1132`.

> The build of the candidate is a job that writes the ref `refs/horch/bundle/<id>`.
> Rescue refs have 2 classes, and fleetd owns both.

Failure scenario: Every bundle and every bisect child writes a unique candidate ref. The bundle then ends in `passed`, `failed` or `dropped`, but no rule deletes or retires its candidate ref. Repeated bundles, rebases and bisects therefore pin all candidate commits and objects. The rescue-ref count and byte budget do not include bundle refs. Repository storage grows until the disk health guard pauses dispatch, and the design gives fleetd no transition that frees the bundle objects. A builder must invent both the owner and the safe deletion point; deleting too early can also remove the candidate before the landing effect is observed.

Smallest change: Make fleetd own bundle refs and define their lifecycle. Keep a candidate through gate and ref-move observation, then delete it after successful landing or after a failed or dropped bundle and all child bundles no longer depend on it. Retry and verify deletion, and report retained bundle-ref bytes.

### T41-3 - HIGH - One job identity cannot support retries and periodic fetches

Lines: `design.md:723-738`, `design.md:1081`, `design.md:1148`, `design.md:1256`.

> A job with the same kind and input key is not run 2 times: its result is used again.
> 1 `git fetch` job runs for each repository on a period.

Failure scenario: A push of feature tip X over remote tip R fails because of a transient network error. The feature row sets a backoff and requires another push, but the next push has the same kind and input key. The unique job rule either refuses the new row or reuses the failed result, so no second network call occurs. The same ambiguity applies to a periodic fetch because its repository and remote inputs can stay unchanged between periods. A builder must violate result reuse or invent an unstated generation in the key. The wrong choice leaves a feature permanently unpushed or leaves remote state permanently stale.

Smallest change: Separate a logical job from its executions. Keep the logical input key for deduplication, add durable attempts with their own ids, and permit a failed push or fetch to queue a new attempt after its backoff or period. Include the repository, subject and relevant configuration hash in the logical identity, and reuse only results whose job kind and freshness contract allow reuse.
