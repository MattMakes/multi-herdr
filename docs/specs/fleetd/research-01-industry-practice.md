## Summary

Research ticket 01, 2026-11-11. Question: how do large engineering organizations land many concurrent changes to the same files? Findings F1 to F31 follow. Each finding has evidence and a label.

Q1, merge queues (F1 to F11):
- Every queue tests a change on top of the target branch plus every change ahead of it in the queue. No queue tests a change in isolation as the last gate (F1, F3, F4, F5, F10).
- On a failure, every queue removes the failing change and rebuilds the changes behind it without that change (F2, F3, F4, F10).
- Batch queues (bors, Mergify, Aviator) split a failed batch by bisection until 1 change fails alone (F5, F8, F9). Google TAP splits a failed post-submit batch into single changes (F7).
- Uber SubmitQueue decides that 2 changes are independent from the build graph, not from file names. 2 changes conflict if they affect a common build target (F6).
- Zuul halves its parallel window after each failure. This limits wasted test runs when failures are frequent (F4).
- Shopify uses a batch size of 8 and drops a PR only after 4 consecutive failures, because of flaky tests (F11).

Q2, test tiers (F12 to F17):
- Google gates a change before it lands with "only fast, reliable" tests, often the unit tests of the project. A change that passes them passes the rest with 95%+ likelihood (F12, F13).
- Google selects tests from the downstream dependency graph. A test is affected if a changed file is in the transitive closure of its dependencies (F14).
- Larger and end-to-end tests run after landing: on release candidates, or on a timer. One Google team runs E2E post-submit every 2 hours (F15, F16).
- Pre-submit tests of single changes cannot detect a conflict between 2 changes that each pass alone (F17).

Q3, stacked changes (F18 to F22):
- jj rebases descendants automatically and never fails a rebase on a conflict. It stores the conflict in the commit (F22).
- Sapling rebases descendants on amend unless that gives a conflict. Graphite `gt modify` restacks and pauses on a conflict for a human (F20, F21).
- Gerrit and Phabricator do not rebase dependent changes automatically (F18, F19).

Q4, less contention on 1 file (F23 to F26):
- Trunk-based development and DORA: branches live hours, merge at least once a day, 3 or fewer active branches (F23, F24).
- Branch by abstraction and feature toggles let incomplete work land on trunk. Both need an abstraction or toggle point in the code (F25, F26).

Q5, numbers (F27 to F31):
- Uber, 9 months of iOS and Android data: 5% chance of a real conflict with 2 concurrent conflicting changes, 40% with 16 (F27).
- Uber: changes only 1 to 10 hours stale have a 10% to 20% chance to break the mainline (F28).
- Conflicts rise with branch duration, number of commits, changed files and non-modular changes (F29).
- AI agent PRs on GitHub (2026): 27.67% conflict rate. 9.9% at a median of 2 changed lines, about 30% at 25 lines (F30).
- Open source textual conflict rate measured directly: about 3.2% of merge scenarios (F31).

## Method

- I read official documentation, company engineering blogs and papers. I extracted PDF text with `pdftotext` and quoted from that text.
- `verified` means that I read the primary source. `not verified` means that I read only a secondary source or a summary.
- Ticket 02 covers merge tools and AI agent systems. Ticket 03 covers this repository. I did not research them here.

## Q1: Merge queues

**F1. GitHub merge queue tests each PR on the latest base plus the PRs ahead of it.** `verified`
- Batch: the queue puts PRs into a `merge_group` with the latest base branch, on a temporary branch with the prefix `gh-readonly-queue/{base_branch}`. Merge limits set a minimum and maximum of PRs merged at once (1 to 100). Build concurrency caps parallel `merge_group` builds (1 to 100).
- Tests: a PR must first pass the required branch protection checks. In the queue, the required checks run again on the merge group. A workflow without the `merge_group` trigger fails the merge.
- Evidence: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue

**F2. GitHub merge queue removes a failing PR and rebuilds the rest.** `verified`
- The failing PR leaves the queue. The queue rebuilds the temporary branches of the remaining PRs without it. A conflict with the base branch also removes a PR.
- Evidence: same URL as F1.

**F3. GitLab merge trains run 1 pipeline per MR on the MRs ahead of it, in parallel, default 20.** `verified`
- Tests: the pipeline of the second MR "runs on the changes of _both_ merge requests combined with the target branch."
- Failure: "If a merge train pipeline fails, the merge request is not merged." GitLab removes it and restarts the pipelines of the MRs behind it.
- Limit: "The default limit is 20" parallel pipelines per train.
- Evidence: https://docs.gitlab.com/ci/pipelines/merge_trains/

**F4. Zuul speculatively tests each change on all changes ahead of it, and halves its window on failure.** `verified`
- Tests: jobs for change B merge A and B. Jobs for E merge A to E. All run in parallel.
- Failure: if C fails, A and B merge, C is reported and dropped. D and E are retested: D against the branch tip, E against D. Zuul cannot tell if D failed because of D or C.
- Window: starts at 20, grows by 1 after each merge. "Each time a change fails, the window is halved."
- Evidence: https://zuul-ci.org/docs/zuul/latest/gating.html

**F5. Bors tests a batch as 1 merge commit on a `staging` branch and bisects on failure.** `verified`
- Batch: "bors creates a merge commit, merging the main branch with all the pull requests that make up the batch." It pushes the commit to `staging`. On a pass, main fast-forwards to `staging`.
- Failure: bors "splits the batch into two batches, and pushes those to the queue." A batch of 1 PR that fails "gets kicked back to the creator so they can fix it."
- Config: `max_batch_size` caps the batch. `pr_status` lists checks on the PR itself. `status` lists checks on the merge commit.
- Evidence: https://github.com/bors-ng/bors-ng and https://bors.tech/documentation/
- Homu: I could not determine the Homu batch and failure rules from a primary source. Uber cites the Rust project as a user of a simple serial queue (F6 source, section 2.2).

**F6. Uber SubmitQueue uses a conflict analyzer on the build graph and speculates on probable outcomes.** `verified`
- Independence: "Roughly speaking, two changes conflict if they both affect a common set of build targets." The analyzer also marks a conflict when both changes together produce a target hash that neither change produces alone. Changes that do not conflict build and land in parallel.
- Tests: all build steps run for each change: compilation, unit tests, UI tests. The paper's guarantee is that all build steps pass at every commit on the mainline.
- Failure: a change lands only if its build steps pass. A logistic regression model predicts success and conflict from features of the change and of the developer. "developers working on the same set of features (or code path) conflict with each other more often."
- Result: before SubmitQueue, the iOS mainline was green 52% of the time in a 1-week view.
- Follow-up (2025): Uber adds build-time prediction so small changes are not blocked by large conflicting changes. CI resource use fell about 53% and P95 wait about 37%.
- Evidence: Ananthanarayanan et al., "Keeping Master Green at Scale", EuroSys 2019, https://www.masoud.io/docs/eurosys19.pdf (sections 1, 2, 5). Follow-up: https://arxiv.org/abs/2501.03440

**F7. Google TAP batches changes post-submit and splits a failing batch into single changes.** `verified`
- Batch: TAP evaluates more than 1 change per second, so "it can no longer run every test on every change." It batches related changes.
- Failure: "TAP automatically splits a failing batch up into individual changes and reruns the tests against each change in isolation." Developers also have culprit-finding tools that binary search a batch.
- Scale (2016 slides): 4.2 million tests, 150 million test runs per day, 13,000+ teams on 1 branch. 84% of pass-to-fail transitions come from flaky tests.
- Evidence: Software Engineering at Google, ch. 23, https://abseil.io/resources/swe-book/html/ch23.html. Slides: Micco, "The State of Continuous Integration Testing @Google", https://research.google.com/pubs/archive/45880.pdf

**F8. Mergify tests a batch as 1 temporary PR, splits a failed batch, and groups PRs that touch similar code.** `verified`
- Batch: `batch_size` sets how many PRs Mergify checks together. After priority and stacks, Mergify fills a batch with PRs that touch similar code.
- Tests: a batch runs on 1 temporary PR. Per-PR checks run only when `batch_size` and `max_parallel_checks` are both 1.
- Failure: Mergify "splits the batch to find the pull request that broke it, removes that one, and merges the others." Batches behind the failed batch are cancelled and requeued. `batch_max_failure_resolution_attempts` caps the splits.
- Evidence: https://docs.mergify.com/merge-queue/batches/

**F9. Aviator MergeQueue tests a batch as 1 draft PR and bisects on failure.** `verified`
- Batch: `batch_size` (default 1) and `batch_max_wait_minutes`. Each draft PR includes the earlier queued changes.
- Failure: the draft PR closes and its PRs are requeued in smaller bisected batches, for example #1-5 becomes #1-3 and #4-5.
- Evidence: https://docs.aviator.co/mergequeue/concepts/batching

**F10. Aviator parallel mode tests each PR on the PRs ahead of it and restarts the queue on a failure.** `verified`
- Failure: "the bot closes all subsequent Draft PRs and restarts the queue after removing the failing PR."
- Evidence: https://docs.aviator.co/mergequeue/concepts/parallel-mode

**F11. Shopify merge queue uses a batch size of 8 and tolerates 3 failures for flaky tests.** `verified`
- Batch: "we went with a batch size of 8 as a balance between throughput and risk." CI runs on 3 batches of queued PRs on a "predictive branch".
- Failure: "on the fourth consecutive failure, we will remove the first pull request failing CI from the queue."
- I could not determine what Shopify does with the other PRs of a failed batch.
- Evidence: https://shopify.engineering/successfully-merging-work-1000-developers

## Q2: Test tiers

**F12. Google runs only fast, reliable tests before a change lands.** `verified`
- "Our general rule of thumb is: only fast, reliable ones." Post-submit can "accept longer times and some instability."
- Each team picks "a fast subset of tests, often a project's unit tests" for presubmit.
- The continuous build moves some presubmit tests to post-submit behind the scenes.
- Evidence: https://abseil.io/resources/swe-book/html/ch23.html (sections "Presubmit versus post-submit" and "Continuous Testing", footnote 8)

**F13. A change that passes Google presubmit passes the rest of the tests with 95%+ likelihood.** `verified`
- "a change that passes the presubmit has a very high likelihood (95%+) of passing the rest of the tests, and we optimistically allow it to be integrated."
- Evidence: same URL as F12.

**F14. Google selects affected tests from the build dependency graph.** `verified`
- "A test is affected iff a file being changed is present in the transitive closure of the test dependencies." (2016 slides)
- Forge and Blaze keep a near-real-time global dependency graph. TAP runs "the minimal set" of downstream tests.
- Presubmit is "typically limited to" the project of the change. Post-submit covers affected targets in all projects.
- Evidence: https://research.google.com/pubs/archive/45880.pdf and https://abseil.io/resources/swe-book/html/ch23.html

**F15. Google runs larger tests on release candidates, after the continuous build.** `verified`
- "As CD builds RCs, it will run larger tests against the entire candidate." The RC goes through a series of test environments, then the same suites run as probers in production.
- Evidence: https://abseil.io/resources/swe-book/html/ch23.html (section "Release candidate testing")

**F16. Published rule for E2E timing: one Google team (Takeout) runs E2E post-submit every 2 hours.** `verified`
- The E2E tests could not run on presubmit because they use test accounts. The team chose "every two hours was a good starting point" instead of the next day's deploy.
- Google also reports that large E2E tests "inherently become broken or flaky." Some teams disable failing E2E tests rather than block development.
- I could not find a general industry rule for when E2E tests run. The rules I found are per team.
- Evidence: https://abseil.io/resources/swe-book/html/ch23.html (Takeout case study and "Challenges")

**F17. Tests of single changes cannot detect a conflict between 2 changes.** `verified`
- Uber: a real conflict exists when each change passes alone and the combination breaks the mainline. "Therefore, pre-submit tests cannot detect these real conflicts."
- Google: "it is possible for two changes that touch completely different files to cause a test to fail." Google calls this a mid-air collision and says it "happens most days."
- Evidence: https://www.masoud.io/docs/eurosys19.pdf (section 1). https://abseil.io/resources/swe-book/html/ch23.html

## Q3: Stacked changes

**F18. Gerrit does not rebase a child change when its parent gets a new patch set.** `verified`
- The UI marks the relation "Not current" or "Indirect descendant", and the change "now needs to be rebased." The Rebase button is a user action. It rebases onto "the current patch set of that other change."
- At submit, the "Rebase If Necessary" strategy rebases the change automatically.
- Evidence: https://gerrit-review.googlesource.com/Documentation/user-review-ui.html and https://gerrit-review.googlesource.com/Documentation/concept-changes.html

**F19. Phabricator records dependencies but does not rebase a stack.** `verified`
- A revision depends on another through a "Depends on Dxxx" line. "after landing, you may need to iteratively rebase stacks of changes." Cascading rebase is an open request (T3875).
- Evidence: https://secure.phabricator.com/Q396

**F20. Graphite restacks dependent branches and stops on a conflict.** `verified`
- "`gt modify` automatically restacks any dependent branches." `gt sync` can leave branches unrestacked. `gt restack` fixes that.
- Conflict: Graphite pauses. The user resolves, runs `gt add .` and `gt continue`, or `gt abort`.
- Limit: from `gt` 1.8.4, Graphite does not modify branches checked out in another worktree.
- Evidence: https://graphite.com/docs/restack-branches

**F21. Sapling rebases descendants on amend unless that gives a conflict.** `verified`
- Descendants are rebased "unless doing so would result in merge conflicts." Then the user runs `sl restack` or `sl amend --rebase` and resolves the conflict.
- Evidence: https://sapling-scm.com/docs/commands/amend/ and https://sapling-scm.com/docs/commands/restack/

**F22. jj rebases descendants automatically and stores conflicts in commits.** `verified`
- "descendants of rewritten commits automatically get rewritten." "the conflicts did not prevent the rebase from completing successfully."
- A conflict is stored as an ordered list of trees. When a conflicted commit is rebased again, canceling terms simplify, so conflicts do not pile up.
- Limit: Git-style diff3 markers "only supports 2-sided conflicts". Sharing conflicted commits with Git users is not advised.
- Evidence: https://docs.jj-vcs.dev/latest/conflicts/, https://docs.jj-vcs.dev/latest/tutorial/, https://docs.jj-vcs.dev/latest/technical/conflicts/

## Q4: Less contention on 1 file

**F23. Trunk-based development keeps branches short and merges to trunk at least every 24 hours.** `verified`
- Developers "collaborate on code in a single branch." CI requires that "all team members commit to trunk at least once every 24 hours."
- A short-lived branch is "the product of a single dev-workstation." For larger work the site names feature flags and branch by abstraction.
- Evidence: https://trunkbaseddevelopment.com/

**F24. DORA: 3 or fewer active branches, branches last hours, merge daily.** `verified`
- "Have three or fewer active branches in the application's code repository." Branches "typically last no more than a few hours." Based on DORA data from 2016 and 2017.
- Evidence: https://dora.dev/capabilities/trunk-based-development/

**F25. Branch by abstraction needs an abstraction layer that all callers use.** `verified`
- Steps: add an abstraction layer, move all clients to it, build the new supplier behind it, switch clients, delete the old supplier.
- Needs: "Use an abstraction layer to allow multiple implementations to co-exist." The system must build and run at all times.
- Evidence: https://martinfowler.com/bliki/BranchByAbstraction.html

**F26. Release toggles let unfinished features land, and need toggle points kept apart from decision logic.** `verified`
- Release toggles "allow in-progress features to be checked into a shared integration branch." They "should generally not stick around much longer than a week or two."
- Code needs: separate toggle points from toggle routing, inject decisions, and use a strategy rather than conditionals for long-lived toggles. Teams cap the number of toggles.
- Evidence: https://martinfowler.com/articles/feature-toggles.html

## Q5: Numbers

**F27. Uber: the chance of a real conflict rises from 5% with 2 to 40% with 16 concurrent conflicting changes.** `verified`
- "with even two concurrent and potentially conflicting changes, there is a 5% chance of a real conflict. This number grows to 40% with only 16"
- Population: Uber Android and iOS monorepos, 9 months. "Potentially conflicting" means the changes touch the same logical parts of the repository. Published 2019.
- Evidence: https://www.masoud.io/docs/eurosys19.pdf (section 1, Figure 1)

**F28. Uber: changes 1 to 10 hours stale have a 10% to 20% chance to break the mainline.** `verified`
- "even changes with one to ten hour staleness have between 10% to 20% chance of making the mainline red." Staleness is the age of the change against mainline HEAD at submission.
- Population: Uber monorepos, 1 year of data. Published 2019.
- Evidence: https://www.masoud.io/docs/eurosys19.pdf (section 2.1, Figure 2)

**F29. Conflict occurrence rises with branch duration, commits, changed files and non-modular changes.** `verified` (abstracts)
- Dias, Borba, Barreto, IST 2020: 73,504 merge scenarios, Ruby and Python MVC projects on GitHub. Conflicts are more likely when contributions touch the same MVC slice, are bigger, involve more developers, commits and files, and are developed over longer periods.
- Menezes et al., SBES 2020: 182,273 merge scenarios, 80 projects, 8 languages. Number of committers, commits and changed files have the biggest influence. Conflicts correlate positively with branch duration.
- These abstracts give no single rate per unit of time or size.
- Evidence: https://doi.org/10.1016/j.infsof.2020.106256 and https://doi.org/10.1145/3422392.3422440 (abstracts read through the Semantic Scholar API)

**F30. AI coding agent PRs on GitHub have a 27.67% textual conflict rate, rising with PR size.** `verified`
- 107K+ PRs from 59K+ repositories. "yielding a conflict rate of 27.67%". By agent: Copilot 15.43%, Cursor 20.06%, Devin 23.04%, Claude Code 26.86%, OpenAI Codex 32.31%.
- Size: median churn 2 lines gives about 9.9%. Median churn 25 lines gives nearly 30%. Medium PRs (46 to 185 lines) stabilize at 32% to 33%.
- Population: simulated merges of agent PRs. Published 2026 (AIware '26).
- Evidence: Ogenrwot and Businge, "AgenticFlict", https://arxiv.org/abs/2604.03551 (abstract, Table 2, Figure 5)

**F31. Measured textual conflict rates in open source are low per merge; the common 10% to 20% figure is quoted from older work.** `verified`
- Vale et al., TSE 2022: 66 projects, 81,005 merge scenarios, 2,608 conflicting. That is 3.2%.
- Ghiotto et al., TSE 2018: 2,731 Java projects, 25,328 failed merges. The paper repeats "10% to 20% of all merges" from earlier studies. 87% of conflicting chunks resolve with code from 1 or both sides. 94% of those chunks are under 50 lines per side.
- Brindescu et al., EMSE 2020, reports that conflicted code that needs manual work is 26 times more likely to have a bug. `not verified`: I read only a search summary, because the publisher page needs a login.
- Evidence: https://www.se.cs.uni-saarland.de/publications/docs/VHF+22.pdf (section 3.1.2). https://leomurta.github.io/papers/ghiotto2018.pdf (abstract, section 3). https://doi.org/10.1007/s10664-019-09735-4

## Gaps

- I could not determine the Homu batch and failure rules from a primary source.
- I could not determine a general industry rule for when E2E tests run. Only per-team rules exist in the sources (F16).
- I could not find a published rate of semantic (non-textual) conflicts outside Uber (F27) and Google (F17).
- I could not find a study that measures conflict rate against the number of concurrent changes to 1 file.

## What the facts mean for options A to E

**Option A, rebase and re-verify.** Every merge queue in Q1 re-tests a change on the newest base before it lands, and rebuilds the changes behind a removed change (F1 to F4). So re-verify after each rebase matches industry practice. A clean textual rebase is not enough: 2 changes can each pass and still break together (F17), and Uber measures 10% to 20% breakage for changes only 1 to 10 hours stale (F28). The check after a rebase must run tests, not only a merge. jj and Sapling show 2 policies for the rebase step: record the conflict and continue (F22), or stop on a conflict (F21). The rebase cost grows with the number of in-flight ideas, because each merge rebases each other branch.

**Option B, combine at the file.** Uber reports that developers on the same code path conflict more often (F6), and Dias et al. report more conflicts for non-modular changes in the same slice (F29). Mergify groups PRs that touch similar code into 1 batch (F8). These facts support grouping work that touches the same code. Against that, conflict rate rises with change size, commits and developers (F29, F30). A combined task is a bigger change. No source in Q1 to Q4 describes a planner that merges requirements from separate features into 1 task.

**Option C, integration lane.** This is the merge-queue model of F1 to F11. Industry gates each landing with fast tests and runs slow and E2E tests later on a batch or on a timer (F12, F15, F16). This matches the operator's position on E2E. A failed batch needs a split strategy: bisection (F5, F8, F9) or split into single changes (F7). Flaky tests force a retry tolerance (F7, F11). Zuul halves its parallel window after failures to limit wasted runs (F4). Uber shows that build-graph independence lets non-conflicting changes land in parallel inside 1 lane (F6). Conflict chance rises with the number of concurrent conflicting changes (F27), so a lane with many ideas on 1 file will see more failed landings.

**Option D, structural merge.** 87% of conflicting chunks resolve with existing code from 1 or both sides, and 94% are under 50 lines (F31). That supports automated resolution of most textual conflicts. A textual or structural merge cannot detect a semantic conflict between 2 changes that each pass alone (F17, F27). Tests must still run after the merge. Ticket 02 covers the merge tools themselves.

**Option E, a mix.** The facts support these parts: a queue that re-tests each change on the newest base (F1 to F4); fast unit tests as the gate for each landing and E2E on a batch or timer (F12, F13, F16); independence from the build or dependency graph so unrelated changes land in parallel (F6, F14); bisection or single-change split on a batch failure (F5, F7, F8); short branch life and small changes to keep conflict rates down (F24, F28, F30); and grouping of changes that touch the same code (F6, F8, F29).
