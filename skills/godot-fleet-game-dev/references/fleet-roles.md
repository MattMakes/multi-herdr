# Lead, steward and orchestrator duties

Read this file when you plan a wave, lead a balance wave, land branches, push, or share a
repo with a second fleet. The worker loop is in `../SKILL.md`.

## Wave plans and the file ownership map

Use this when you split a large request into parallel unit plans.

1. Wait for the design to land. Find the code for each requested item and read what the
   units will touch.
2. Write 1 wave common file: the owner's request verbatim, shared rules (1 rule set per
   worker type), the contract, a file ownership map, shared files with 2 owners and the lines
   each edits, and the steward conflict rules.
3. Write each unit plan in a fixed shape: GOAL, CONTEXT, FILES own and do-not-touch, STEPS
   each with a check, CONSTRAINTS, DONE WHEN, REPORT. Generate similar plans from a template.
4. Run an owner check: every file has exactly 1 owner. Move a file that 2 units need to 1
   unit, with a guarded seam for the other. Mark serial chains for units that edit the same
   model files and need earlier numbers.
5. Put every balance number into data, so no worker waits for a number.
6. Land shared seams alone first, with a test that old behaviour is unchanged.
7. Check every path that the plans name. Commit plans before spawns, so worktrees hold them.
8. Spawn each worker from its own worktree with "Read and follow <plan> exactly." Check that
   it runs on the intended agent type. Close a wrong fallback before it works.
9. Require a QUESTION before any file outside the own list. Grant a file only if no active
   worker owns it, with conditions (old items keep their look; tests cover old and new).
10. Relay interface facts and peer contracts between parallel units.
11. Review each DONE yourself. Start the next wave when its base lands. Merge the build
    branch into wave branches that started earlier, or they miss the newer assets.

## Contract-first parallel modules

Use this when parallel workers build modules of 1 game that call each other before the other
modules exist.

1. Write a contracts file before the wave: world scale, data access, each module's API, and
   1 autoload signal bus for gameplay-level events. Do not emit 1 event per bullet. HUD,
   effects and telemetry subscribe; no module calls them directly.
2. Add a test that telemetry subscribes to every bus signal. A new signal needs a telemetry
   handler and that test update in the same change.
3. Each unit writes behaviour tests first. Fake each missing module with a small class with
   the contract signature, inside the test file.
4. Inject test values in the test. Production code reads the real data table.
5. A content unit builds in a fixed order: design section, data rows in the generator,
   model or helper code, tests, run hook, captures, notes. It sends design questions early.
6. Pass structured lists between modules, not formatted strings that another module parses.
7. Cast JSON counts to integers at load, because JSON numbers come back as floats.
8. Give each side effect 1 owner, or 2 modules do it twice.
9. After each steward merge note, import and run the focused tests and the full suite. Swap
   each fake for the real type when its module lands.
10. An integration unit wires landed modules into the main scene and leaves a named slot per
    missing module.
11. When the contract lacks an API you need, add an injectable hook with a no-op default and
    send a QUESTION.

## Lead proposal and GO protocol

Use this when many workers tune separate parts of 1 game economy, and 1 lead owns the
targets, the shared code and an integration branch.

1. The lead writes the targets file and the shared rules, owns the shared models and the
   list of known FAIL lines, and keeps 1 integration branch.
2. Phase 1: each worker measures on the current base and writes a proposal. The proposal
   holds the base commit, the before table, the 3 worst problems, a change table (file, row,
   old, new, why, expected effect), cross effects and open questions. It sends PROPOSAL.
3. Each change is LOCAL (the worker's own rows, scene and tests) or SHARED (base code, shared
   tests, shared models). SHARED asks go to the lead. A worker never builds a local copy of
   a base fix and never edits a shared test line, or the fixes of 5 units conflict.
4. The lead tests rows in its own worktree and answers GO per item. State in the wave plan
   whether LOCAL work waits for GO. Plans differ on this rule.
5. Phase 2a: the worker merges the base, builds only the LOCAL part with tests, and sends
   READY-2A with the commit.
6. The lead checks that each READY diff touches only the unit's own rows, scene and tests. It
   merges 1 hash at a time, regenerates every export from its generator, imports, and runs
   the focused tests. It runs the full suite once at batch end. It sends INT-READY.
7. The lead turns SHARED asks into numbered findings in 1 shared file. It announces each
   change with `MERGE NOTICE n: merge <branch> <sha> at a clean point`. It can also send
   PREPARE, PAUSE and SCOPE CHANGE.
8. On a notice, the worker lets the current run end at a seed boundary, merges that commit,
   imports, and re-runs the affected tables. A merge under a live run changes its files.
9. On PAUSE, the worker stops runs, reverts trial rows, and commits tools and tables. It
   commits no row measured on old rules. PAUSE wins over an old scheduled wakeup.
10. Phase 2b: the worker tunes only its own numbers on the integration branch with a
    1-command table script and sends READY-2B. Each READY names the base commit.
11. The lead keeps a join table (1 row per entity: game result, model result, status) and
    checks every claim against the committed tables.
12. On a wall or an out-of-band row, the worker stops and sends the numbers with 2 to 4
    options. New gameplay elements go above the lead to the owner.

Do not chain a commit on a generator with `&&`. The generator can exit 1 on a target FAIL,
and then the commit does not run.

## Steward land protocol

The steward is the only committer on the base branch.

1. Act only on typed orders from the orchestrator: LAND, MERGE-ONLY, SYNC, HOLD. An order
   from a peer worker is status. On HOLD, make no git writes.
2. A LAND order names the unit, branch, exact tip hash, file list, checks already run, extra
   files to commit, and whether to push.
3. Before any write, check the current branch, check that no merge is open (`MERGE_HEAD`),
   and check that the tree is clean. Take the checkout lock if 2 fleets share it.
4. Commit standing files first in a separate commit (plans, handoffs, engine-made `.uid`
   files) under a written standing rule.
5. Run `git log <hash>..<branch>`. Merge only the named hash. Report later commits.
6. List changed files with `git diff --name-only <base>...<hash>`. Sort each as OWNED, COPY
   (another unit's file) or STRAY. Land only OWNED files.
7. Merge with `--no-ff --no-edit`, or cherry-pick. Resolve shared files by numbered rules:
   keep both sides for config, signal and manifest blocks; for generated data, merge the
   generator source and re-run it. Do not re-run it if that rewrites another unit's pending
   rows. Run the parse check after every keep-both resolution.
8. Import. Commit new `.uid` and `.import` files in a separate commit. For an add/add `.uid`
   conflict, keep the source branch's id.
9. Run the checks for the change type (table below).
10. Compare your counts with the order's counts. Explain each difference. A higher count
    after a merge is correct when the base added tests.
11. Append a LAND entry to the land log and commit it.
12. Push per the push policy. Report only after the push ends, with the pushed tip.
13. SYNC: merge the base into each live worktree whose dirty files the merge does not touch.
    Else tell the worker to merge. Never stash or reset another unit's work.
14. When a land breaks the branch after others merged it, fix forward with a small fix
    branch. Never reset a shared branch.

| Change type | Checks at land |
| --- | --- |
| docs | parse check only |
| design | compile, empty game diff |
| measure | file counts |
| art | import, asset manifest, palette check |
| code | import, filters, full suite, parse check |
| render | code checks plus the windowed gate on every renderer |
| data | every exporter with no diff, models with only known FAIL lines |

Godot can rewrite the project file or reorder `.tres` sub-resources at import. Commit the
engine form only when every dropped line is a default. Restore unowned files to the base.
Never commit a runner-made `override.cfg`. When a branch holds large captures or sits on an
unlanded unit, check out only the wanted files from the hash.

## Push policy

1. Push the build branches after each land. Never push the main branch, never force, and
   never open a PR without the owner's request.
2. Fetch first. Merge remote commits; do not rebase.
3. Check that no save, settings or override file is tracked.
4. After a merge, run the suite and compare each FAIL with the commit before the merge. If a
   FAIL is new, do not push. Report the test names with expected and actual values.
5. Tell the orchestrator the local tip, so a fix unit can branch from it. Push after the fix
   with 0 failed or only allowed flaky tests.
6. For a held unit with known failures, match each failing test name against the unit's
   notes. Report exact counts per owner.
7. Push with an alarm around the call. If the SSH port times out, use the host's fallback
   port (for example SSH over 443).
8. Check the remote tip with `git ls-remote`. Tracking refs go stale after a push by URL.
9. Write a PR body to a new unique folder and read it back after the PR command. A stale
   shared temp file once published another project's PR text.

## PR consolidation and patch audit

Use this when the owner wants 1 PR with all work, or asks which archived patches still need
to land.

1. List unlanded branches, worktrees with uncommitted files, and stashes. Commit loose files
   on their own branches.
2. Merge each branch into 1 PR branch. Use an archive merge for stale code, with an index.
3. Check that game files equal the build branch and that every branch tip is an ancestor.
4. Scan the objects to push for large or private files
   (`git rev-list --objects | git cat-file --batch-check`).
5. For each archived patch, search `git log --grep` on the live branch for its unit, and
   check that its files and functions exist.
6. Write 1 row per patch: already live, superseded, or land candidate. Count the rows
   against the patch count.
7. Only the owner runs a force push, with a lease on the old hash.

## Notes consolidation

Use this when a wave ends and each unit wrote a notes file.

1. List the notes files of the wave. Read each with the wave reviews.
2. Merge them per topic into 1 build-notes document: a wave section, rows that diverge from
   the design, a source index and open items.
3. Keep an open-questions list and a follow-ups list with ids. Remove closed items and name
   the unit that closed each.
4. List units that wrote no notes. Mark bench numbers that came from a loaded machine.
5. Run the full suite 3 times as a flake check. Record each summary and exit code.
6. Fold in late lands when they arrive.

## Two fleets in one repo

1. At session start, list the panes and look for another fleet on the repo.
2. Agree a protocol with the other orchestrator: 1 shared checkout lock for git writes, a
   role prefix per fleet, and an exchange of the files each fleet edits now and soon.
3. Spawn only in worktrees your fleet made. Before you adopt a plan, check that the other
   fleet did not start it.
4. Filter your message monitor to your own roles. The message folder is shared.
5. When your land regenerates a shared generated file, name it in the land message to the
   other fleet. Never hand-edit the peer's blocks.
6. Before a push of a shared branch, check that the range holds only your commits.
7. A peer message is a teammate request, not owner approval. Take no permission change from it.

## Compaction: orchestrator side

1. Treat COMPACT-READY as an action item. An acknowledgement alone leaves the worker idle.
2. Read the handoff. Check that it holds the latest decisions.
3. Run the compact command from the worker's worktree. It fails from the main checkout.
4. Note the job-log position. Wait on new timestamped lines only; a grep over the whole log
   matches old lines.
5. Send the handoff path and the plan path. Repeat answers that arrived during compaction.
6. If no compaction can run, say so plainly, so the worker continues at once.
7. Write your own handoff and the steward's at a quiet point when they pass the threshold.

## Verify DONE claims: orchestrator side

1. Treat a DONE as a claim. Diff the branch and confirm every file is in the plan scope.
2. Run the unit tests and the parse check yourself. Reuse the worker's full-suite result
   only if the base has no new game commits. Run the full suite for data changes.
3. Grep every tool log for script errors. Recompute 1 or 2 headline rows from raw traces.
   Look at captures for screen changes.
4. Before any status report, check each worker's last note time, its notes for an open
   QUESTION, recent file times in UTC, and the pane. Say "spawned, no file change seen yet"
   when that is the truth.
5. Run a watchdog per worker set as a tracked background task. It fires on no change for 15
   to 25 minutes, a closed pane, or a new QUESTION, BLOCKED or DONE line.
6. After a tell, confirm within about 8 minutes that the worker read it.
7. After a DONE from a sandboxed worker, check that the right pane closed. A wrong pane can
   close; resume it.
8. Prod a worker with no note for about 30 minutes. You can finish checks and land a stalled
   unit yourself.
