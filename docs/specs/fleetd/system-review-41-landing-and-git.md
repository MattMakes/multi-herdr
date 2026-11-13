# Systems review 41: landing, git and no-loss rules

Scope: `docs/specs/fleetd/design.md` at `043cc279b99c951bdadf6adf5211df96eca6f156`, with the amendments, prior design reviews, preservation records, and current landing compare-and-set behavior checked for context.

Validation: the ticket Check command and `git diff --check` pass.

## Verdict

NOT SOUND

The design has 2 CRITICAL, 5 HIGH and 1 MEDIUM findings under this lens. The worktree rescue path is careful, but the migration order cannot build the landing model as written, and an intake race can strand an accepted idea after its feature merges. Bundle recovery, branch mutation, stack retirement, invariant tests, rescue retention and external integration outcomes need explicit contracts before implementation.

## Answers

**1. No-loss invariant checks and tests.** All 6 invariants are checked by `check_invariants(tx)` after bundle landing and feature integration on each reconcile pass (`design.md:984-1010`), and I8 to I13 are defined at `design.md:655-660` and said to be checked against git at `design.md:913-922`. I8 has only the partial Phase 0 bundle test, which does not prove content equivalence or squash detection; I9, I10, I11 and I12 have no direct requirement, failure-mode or phase-gate test; these 5 invariants therefore have no complete test (`design.md:1259-1266`). I13 has the FR-2 property test for exactly 1 sub-orchestrator per feature (`design.md:134`), although Phase 6 does not name I13 in its gate.

**2. Bundle and bisect state.** A task branch moves from `ready` to `in_bundle`; after a passing gate it becomes `landed`, and after a failure the isolated failing task becomes `gate_failed` while the others continue (`design.md:890-901`). The exact states of branches in each bisect partition, candidate identity, completed bisect work and crash recovery are silent; the general restart rule only says that fleetd rebuilds from the ledger and files (`design.md:156`, `design.md:1223`). A branch row has one landing state, but there is no bundle row or bundle id, so the design does not prove that one branch cannot belong to 2 live bundle attempts (`design.md:588-615`).

**3. Feature branch movement and compare-and-set.** fleetd lands bundles, resolver tasks land commits through task branches, continuous rebase rewrites feature history, and the operator can move the primary branch by merging; the branch owner field also permits `worker`, `fleetd`, `sub-orchestrator` or `operator` (`design.md:588-600`, `design.md:901`, `design.md:928-934`). I9 says feature and primary branches move only by fast-forward, while I10 anticipates forced updates for rebases (`design.md:655-657`). The `branches` table records no observed tip, and the design states no compare-and-set against a ledger tip for any move; only gate runs record candidate and base tips (`design.md:602-609`).

**4. Continuous rebase and running work.** When the primary branch moves, fleetd rebases feature branches from the bottom of each stack, reruns the fast gate, and creates a resolver task on conflict (`design.md:927-931`). The treatment of running task branches, their `base_sha`, and active worktrees is silent, as is whether their later landing must rebase from the old feature tip. The design says `rerere` is on and supplies conflicts in `zdiff3` form, but it does not say that fleetd sets `rerere.enabled` and `merge.conflictStyle=zdiff3` or verifies their effective values (`design.md:928-929`).

**5. Intake during gates and pull requests.** `--to` accepts an idea until the feature pull request merges, so it accepts one during a full gate and while the pull request is open (`design.md:882-884`). A later task landing moves the feature tip, makes the earlier full-gate pass stale, and requires another full gate (`design.md:931`). The design is silent on preventing a gate pass and operator merge while the newly accepted idea is still clarifying or ticketing and has no task branch yet.

**6. Manual pull requests.** The waiting count is unbounded because there is no cap on features and the default merger is the operator (`design.md:779`, `design.md:933`); this is the D8 decision. Squash merges and merges with extra commits are learned from git by content rather than from the GitHub API (`design.md:655`, `design.md:917`, `design.md:934`). A pull request closed without merge is silent because manual mode stores only a pull request or compare URL and the design defines no closed-unmerged observation or transition (`design.md:505-513`, `design.md:933-934`).

**7. Stack retirement.** A real conflict sets the younger feature on the older feature, and a stacked pull request uses the older branch as its base (`design.md:927-933`). If the older feature is abandoned, the design is silent on clearing `stacked_on`, changing the pull request base, or rebasing the younger feature. If the older feature is squash-merged, the primary branch move triggers continuous rebase in general, but the design is silent on removing the stack edge and assigning ownership of the younger rebase.

**8. Rescue ref retention.** fleetd creates and verifies rescue refs and lists never-landed refs in the closing note (`design.md:848-867`). The only retention statement says rescue refs sit beside archive and trash refs under the same rules, while the preservation table says only trash refs expire after 30 days (`design.md:869`, `design.md:1095`). The design is silent on which class a rescue ref uses, who deletes it, when deletion is safe, and what bounds the git object bytes retained; the 20 GB budget covers worktrees, not retained objects (`design.md:865`).

**9. Other repository shapes.** A different primary branch name is supported through `features.primary_branch` and the explicit project base-branch rule (`design.md:505-510`, `design.md:873-875`). Protected primary branches are supported only in the sense that agents do not merge them and the operator merges by default (`design.md:66-70`, `design.md:163-170`, `design.md:933`). A rejected push is only time-bounded, and a repository with no remote is silent; neither case has a feature state, retry rule, local-only mode or escalation contract (`design.md:905-910`, `design.md:933`).

## Findings

### S41-1 - CRITICAL - Phase 4 requires branch state that Phase 6 creates

Lines: `design.md:1264-1266`, `design.md:1276-1277`.

> Phase 4 adds the 2 declared gates and the landing state machine.
> Phase 6 adds the feature level and the `features`, `branches`, `gate_runs` and `trial_merges` tables.

Failure scenario: Phase 4 verifies a task and invokes the required landing state machine. The feature row, feature branch lifecycle, task-branch landing rows and gate-run tables do not exist until Phase 6, so the task cannot enter `ready`, `in_bundle` or `landed` as designed. The Phase 4 gate can pass FM-2, FM-6 and FM-9 while the designed landing path cannot run, and the operator sees verified tasks that cannot complete integration.

Smallest change: Move the minimum feature lifecycle and the `features`, `branches`, `gate_runs` and bundle schema into Phase 4, or defer the landing state machine and both gates to Phase 6 and change the Phase 4 claims and gate accordingly.

### S41-2 - CRITICAL - An accepted follow-on idea can be stranded by feature merge

Lines: `design.md:771-779`, `design.md:882-884`, `design.md:931-934`.

> A feature is an intake funnel until its pull request merges.
> The full gate runs once for a feature, when its task branches are landed.

Failure scenario: An operator adds an idea with `--to F` while F's full gate runs. The new idea is still clarifying and has no task branch, so the existing tip passes, the operator merges the pull request, and fleetd marks F `merged`. I13 then requires no sub-orchestrator for F, while the accepted idea still belongs only to F and cannot finish planning or reach the primary branch. The operator sees a captured or clarifying idea with no live owner, and its work stops for good.

Smallest change: Define an intake generation for each full gate. A feature may become `ready` or `merged` only when every accepted idea in that generation is `integrating`; a later accepted idea must invalidate readiness and prevent merge recognition until its work lands.

### S41-3 - HIGH - Bundle bisect has no durable identity or crash-recovery state

Lines: `design.md:588-615`, `design.md:901`, `design.md:1003-1008`, `design.md:1223`.

> On a failure, fleetd bisects the bundle, sends the failing task to `gate_failed`, and the others go on.

Failure scenario: Branches A, B and C enter one bundle and its candidate fails. fleetd records all 3 as `in_bundle`, starts a bisect partition, and crashes after one partition passes. On restart, the ledger has no bundle id, member table, candidate ref, partition queue or completed partition result. One implementation leaves all 3 branches stuck; another rebuilds a second candidate and can run A in 2 bundle attempts or forget C. The operator sees branches that never land or a candidate whose gate did not judge its exact members.

Smallest change: Add durable bundle, member and bisect-partition rows keyed by candidate tip and base tip. Give each task branch at most 1 active bundle membership and define idempotent restart transitions for each partition.

### S41-4 - HIGH - Branch mutation has no recorded-tip compare-and-set contract

Lines: `design.md:588-609`, `design.md:655-657`, `design.md:901`, `design.md:927-934`.

> A feature branch and the primary branch move only by fast-forward to a candidate that passed its gate.
> Before each rebase or forced update of a branch, a rescue ref of the old tip exists.

Failure scenario: A gate passes for feature tip X over base P. Before fleetd updates the ref, the operator or a resolver moves the feature branch to Y. Because the branch row stores no observed tip and the update rule states no compare-and-set, fleetd can update from stale X, overwrite Y during a rebase or push, and keep only a rescue of X. The operator's commit at Y is not in the rescue and disappears from the feature ref.

Smallest change: Store the last observed local and remote tip on every branch row. Require `update-ref <new> <expected-old>` locally and `--force-with-lease=<ref>:<expected-old>` for rewritten remote refs, and make a mismatch reopen the gate without moving either ref.

### S41-5 - HIGH - A stack has no transition when its older feature ends

Lines: `design.md:505-513`, `design.md:927-934`.

> fleetd puts the younger feature on top of the older one.
> A stacked feature's pull request has the lower feature's branch as its base.

Failure scenario: Feature B stacks on older feature A. The operator abandons A, but B keeps `stacked_on=A` and its pull request base remains A, so B either includes changes the operator rejected or cannot merge. In the squash case, A's commits are absent from primary; without an explicit unstack rebase, B's pull request can reintroduce A's commits or remain based on a branch that fleetd later deletes. The operator sees rejected work return or B stay blocked indefinitely.

Smallest change: Add stack-edge states and an owner. On older merge or abandonment, rescue B, clear or retarget `stacked_on`, rebase B onto the new effective base, rerun both gates, and update or recreate the pull request base.

### S41-6 - HIGH - Five no-loss invariants have no complete proof test

Lines: `design.md:655-660`, `design.md:913-922`, `design.md:1259-1266`.

> fleetd checks these on every pass against git.
> Phase 0 adds a bundle test and a conflict test, but no gate names I8 to I12.

Failure scenario: A builder implements squash detection with ancestry only and forced-update rescue without verifying the saved tip. All named phase gates still pass because none exercises I8 or I10. The operator squash-merges a feature, fleetd never recognizes it, and a later forced update loses the only old tip because the unverified rescue path is also untested. The design's no-loss claim ships without an executable proof.

Smallest change: Add one fault-injection or property test for each I8 to I12 and put those tests in the phase gate that first enables the invariant. Include squash with conflict resolution, stale gated tips, crash before and after rescue, orphan branch adoption, and stash-blocked deletion.

### S41-7 - HIGH - Rescue retention can either delete unique work or grow without bound

Lines: `design.md:861-869`, `design.md:1095`.

> A closing note lists any rescue ref that never landed.
> Rescue refs sit beside archive and trash refs under the same retention rules.

Failure scenario: An implementation treats every rescue as a trash ref and `ledger gc` removes it after 30 days, even though the closing note identifies it as work that never landed. That ref is the only saved copy after its worktree was removed, so the operator's later restore loses paid-for work. If the builder instead treats every rescue as permanent archive, repeated rebases and retries pin unbounded git objects outside the worktree disk budget and can fill the disk.

Smallest change: Classify rescue refs explicitly. Keep a unique never-landed rescue until operator disposition; expire superseded old-tip rescues only after merge and a stated retention period; make fleetd or `ledger gc` the owner; report and bound retained git object bytes.

### S41-8 - MEDIUM - Push and manual pull-request failures have no lifecycle state

Lines: `design.md:505-513`, `design.md:905-910`, `design.md:933-934`.

> `manual` pushes the feature branch and shows the compare URL.
> fleetd reads merged state from git, not from the GitHub API.

Failure scenario: A project has no remote, a remote rejects the push, or the operator closes the pull request without merging. fleetd has no `push_failed` or `closed_unmerged` feature state, retry rule, due time or escalation rule. The feature remains `ready` or `open` with no usable pull request, and an unbounded number of such features can wait without a terminal outcome.

Smallest change: Validate remote capability in project configuration and add explicit push and manual-PR outcome states with bounded retry and escalation. Support a declared local-only mode, and require an operator command for closed-unmerged manual pull requests when no API is used.

## Checked and sound

- Worktree rescue uses a separate index, includes tracked and untracked non-ignored files, verifies the rescue commit against disk, and leaves the tree in place on verification failure (`design.md:848-863`).
- Gate results record both the candidate tip and base tip, and a pass becomes stale when either moves (`design.md:602-609`, `design.md:931`).
- The primary branch name is project data, not a hard-coded `main` assumption (`design.md:505-510`, `design.md:873-875`).
- The retained current land behaviors cover ownership under the land lock, reset after a red land, tree-specific step validity and bounded git network calls (`design.md:905-911`).
- Unknown fleet branches are adopted and reported instead of deleted (`design.md:658`, `design.md:920`).
- Disk pressure removes only eligible landed or rescued trees and pauses dispatch before touching an active tree (`design.md:865`).
- The default integration boundary leaves protected-primary merging to the operator (`design.md:66-70`, `design.md:163-170`, `design.md:933`).
