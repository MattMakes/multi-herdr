# Fleet pitfalls: the full table

Read this file when a fleet step fails and the short table in `../SKILL.md` does not name
the fault. The rows are grouped by area. Each row comes from a real fleet session.

## Worktrees, imports and merges

| Symptom | Cause | Fix |
| --- | --- | --- |
| 30 to 50 tests fail with "Identifier not declared", "Compilation failed" or a missing compressed texture. | Fresh worktree, or a merge brought new class names, images or sounds; the `.godot` cache is stale. | Import after every merge and in every fresh worktree, before any test. |
| `godot --check-only` says "Identifier not found" for an autoload. | `--check-only` does not load autoloads. | Use the project parse check or the suite. |
| A Godot run rewrote the bus layout or the project file. | The editor or runtime saves engine defaults. | Restore unowned files before a commit. |
| The diff against the base lists another branch's files. | That branch landed first. | List the files of your own commits. |
| A merge refuses on an untracked `.uid` or plan copy. | The import made a local copy of an incoming file. | Compare with the incoming file, delete yours, merge again. |
| The same conflict in a shared file comes back on each merge. | The owner holds the newer copy. | Take the owner's side each time. |
| An amend after a stopped merge renamed your own commit. | A merge was still open. | Fix the message; check for `MERGE_HEAD` before any commit. |
| A commit completed an open no-commit merge, with your draft inside. | Commit while `MERGE_HEAD` existed. | `git merge --abort` first; restore draft files before the merge. |
| `git apply` of a whole patch applied nothing. | 1 file failed. | Apply per file with `--include`. |
| 9 new failures after a merge. | Unknown: the merge or the base. | Run the base alone in a detached worktree. |
| A test hard-coded the old value of a changed constant. | The test pinned data. | Ask the owner of the test; update only the broken asserts. |
| A test used a helper that does not exist, or a typed cast failed to parse. | Guessed helper names. | Read the test base for real names; use a `Variant` local. |
| A capture tool sat in the test folder and ran as a test. | Wrong folder. | Move non-tests to the review folder. |
| `git diff --check` showed hits from other units' files. | Check ran on the whole tree. | Check only your own files. |
| A steward merged other work into your branch during a timing run. | Shared branch moved. | Note which runs include it; run the pairs again. |
| A new worktree lacked its plan. | The plan was untracked when the worktree was made. | Commit plans before the worktree is made. |

## Shared machine, runs and locks

| Symptom | Cause | Fix |
| --- | --- | --- |
| Load average 50+ while the owner played; hung windows loaded the machine far more. | 20+ checks at once, no `nice`, runs that did not end. | Slot runner, `nice`, 1 process per worker, runs that end by themselves. |
| 1 call waited about 10 hours. | The slot wait sat inside the alarm, or outside it with no bound. | Alarm on the Godot command only; check the runner state after a long gap. |
| The alarm stopped the parent, but the Godot child kept running. | Alarm around the runner. | Find and stop the child; check slot owners. |
| `timeout` gave exit 127. | macOS has no `timeout`. | The perl alarm, `gtimeout`, or `--quit-after`. |
| A `ps` or `pgrep` match was your own command line or the grep. | Name match. | Match the Godot binary and the worktree path; read each match. |
| A run started with a shell `&` was not tracked. | Untracked background job. | A tracked background task with an end marker, or the foreground. |
| After a shell stop the batch still ran and held a slot. | Child survived. | Check for leftover processes before a new run. |
| A suite started before a `nice` rule. | Rule came late. | Renice the running process; do not restart it. |
| A load figure was read as a count of games. | Load is not a count. | Trace processes; the slot cap is the real limit. |
| The runner failed on a list of commands. | It takes 1 command. | 1 call per run. |
| A comma-separated test filter ran 0 tests and looked green. | The runner takes 1 filter. | 1 filter per call; check the test count is not 0. |
| A shared tool settings file had reduced motion on, and event ticks moved. | Shared user settings. | Pin the settings a tool depends on in its own slot. |
| A long run without output looked like a stopped worker. | No progress signal. | Send a note with the job and its expected time. |
| A release deleted another worker's lock; 2 timing runs overlapped. | `mkdir L && run; rm L` ran the `rm` after a failed `mkdir`. | Release only in the success branch with an owner check; report at once and remeasure. |
| A lock had no owner file. | Old helper or a crash. | Leave it; match its time with other runs to find the owner. |
| A quiet window ended early, but a script still waited by the clock. | Clock-based wait. | Restart it without the quiet flag. |
| A gate job waited forever on 1 window slot. | Fixed slot. | Take whichever slot is free. |
| A lock helper gave up when both locks were busy. | Wait too short. | Wait longer in small steps. |
| A visual gate opened windows without a lock. | Gate outside the lock. | Run the gate under a window lock too. |
| A merge during a frame-cost job spoiled the timing. | Files changed under the job. | Stop the job, merge, start again. |
| A running chain used the old lock script after a rule change. | Chain started earlier. | Restart chains after a rule change. |
| Another worker's hung capture ran 23 minutes. | Not yours. | Report it; do not stop another worker's process. |
| An orphan capture held a runner slot for 7 hours. | Parent gone. | Check the parent is gone, then stop it. |

## Long waits and monitors

| Symptom | Cause | Fix |
| --- | --- | --- |
| The harness moved a long command to the background, and batches ran on unseen. | Call longer than the tool limit. | Chunks, or a known background job with a wait. |
| A chunk was cut by the alarm (exit 142). | Chunk longer than the alarm. | Smaller chunks. |
| A foreground `sleep` was blocked by the harness. | Harness rule. | A monitor on an end marker. |
| A monitor woke the agent on every log line. | Unanchored pattern matched a common word. | Anchor the pattern, for example `^==`. |
| A monitor expired after 15 to 30 minutes. | Monitor lifetime. | Re-arm it on the same file. |
| A monitor sent the same 0-of-3 event many times. | Fires per poll. | A monitor that fires once when all lands show. |
| A filtered run hung at 100 % CPU with no output. | No time limit, no log. | Kill it; re-run with a log file and a timeout. |
| A parse check hit its limit with no summary. | Run cut. | Read the log to find where it stopped. |
| A worker polled `git log` with `sleep` about 80 times. | Busy wait. | End the turn; let the reply arrive. |

## Questions, waits and scope

| Symptom | Cause | Fix |
| --- | --- | --- |
| A worker idled for 50 minutes; another for about 6 hours. | The question went only to the ledger, or the message failed. | Tell plus a notes-file copy. |
| A second test outside FILES broke after the first approval. | Approval was per file. | Ask again; do not edit until approved. |
| A message to a closed peer failed. | Peer pane gone. | Send it to the orchestrator. |
| A read-only command after a helper was denied as a bypass. | Permission rules match the literal form. | Stop and explain; do not change the form. |
| A worker deleted tests of a removed screen and lost coverage. | Delete instead of adapt. | Restore and adapt them. |
| A plan missed a third file with a hard-coded table. | Plan gap. | Ask; replace the table with a rule. |
| A worker asked twice about the same class of test. | No standing rule recorded. | Record the answer as a standing rule. |
| A grep matched a plan-only commit, not the merge. | Loose match. | Match the merge subject; check the work landed. |
| A dependency merged the base itself, so no merge subject appeared. | Fast-forward or self-merge. | Check the branch tip or ask. |
| A stub was not committed on the base. | Missing land. | Report it; remake the worktree after the land. |
| After a peer rebalance, a fixture held a removed node and tests passed on old data. | Stale fixture. | Rebuild the fixture from the new data. |
| A compatibility gate alone did not stop old tests failing after the land. | Tests knew only 1 order. | Give old tests pre-land and post-land branches. |
| The agent did not re-run tests on a partial commit. | Assumed the full branch result. | Run them, or report that fact. |
| A pasted owner message ran as a shell command. | Paste into the wrong place. | Nothing ran; keep waiting for the real GO. |
| A worker sent BLOCKED for a planned wait. | Wait read as a block. | Report `ready` and wait. |

## Compaction and handoffs

| Symptom | Cause | Fix |
| --- | --- | --- |
| The worker sat idle about 1 hour after COMPACT-READY. | The orchestrator only acknowledged it. | Run the compact at once. |
| A wait on the job log fired early. | Grep over old lines. | Wait on new lines only. |
| The compaction failed from the main checkout. | Wrong folder. | Run it from the worker's worktree. |
| A worker waited for a compaction that could not run. | No live record. | The orchestrator says "do not wait"; the worker continues. |
| A resume note repeated an answer the handoff lacked. | Answer arrived after the handoff. | Add it before the compact; act on the union. |
| Helper scripts were lost after compaction. | They lived only in `/tmp`. | Copy them into the review folder before the handoff. |
| A batch held a timing lock at compaction time. | Run left alive. | Stop the parent; let the take finish and release. |
| A run of the role was still alive after compaction. | Background job. | Check processes; read the job output; do not re-run it. |
| The handoff stayed in the final diff. | Not cleaned. | Delete it before the final scope check if the plan does not own it. |
| A worker answered "are you done?" with DONE while GO was pending. | Ambiguous question. | Answer with the open steps. |

## Claims and reports

| Symptom | Cause | Fix |
| --- | --- | --- |
| The orchestrator reported "running now" for a blocked worker. | Status from memory. | Check note times, open questions and file times in UTC. |
| File times from `ls` read as UTC. | `ls` shows local time. | Print UTC times. |
| A pane showed "idle" while the worker waited on a long command. | Pane state lags. | Read the pane and the newest log time. |
| A watchdog started with `&` could not wake the agent. | Untracked job. | Start it as a tracked background task. |
| Tests passed, but the bot bought in data order for its first 28 buys. | Tests did not check the behaviour. | Recompute rows from traces before a land. |
| A count in a report was 12, not 13. | Written from memory. | Check each claim with a script. |
| A lead claimed a gap closed when the tables showed it did not. | Claim not checked. | Check claims; correct at once. |
| The orchestrator stated +200 % where data gave +100 %. | Copy error. | State the true value in DONE. |
| A tool README claimed a wrong fact. | Unchecked doc. | Correct the README. |

## Shell and edits

| Symptom | Cause | Fix |
| --- | --- | --- |
| An unquoted variable with `-s path --` became 1 argument, so Godot started the main scene. | zsh does not split words. | Pass explicit arguments or an array. |
| `sed -i` failed with "extra characters at the end of g command"; the chained commit did not run. | BSD `sed -i` needs `''`. | `sed -i ''` or a Python edit, then grep the value. |
| `sed -i -E` made `-E` backup files. | `-E` read as the suffix. | `perl -pi -e`; remove the backups. |
| `--include=*.gd` gave "no matches found". | zsh expanded the glob. | Quote it. |
| `echo $?` after a pipe showed the status of `tail`. | Pipe status. | A log file or `pipefail`. |
| A shell alias `c` broke a function. | 1-letter names. | Rename. |
| A heredoc inside `$(...)` in bash 3.2 broke on quotes. | Old bash parser. | A script file. |
| A `git grep` claim was wrong. | `\|` is not "or" in the default syntax. | `-E` or several `-e`. |
| A `python3 -I` check could not import a user-site package. | `-I` hides user site. | Use a venv or run without `-I` from a separate tool folder. |
| An edit assert fired on a line with 3 tabs, not 4. | Wrong indentation in the anchor. | Check with `cat -et`; run the edit again. |
| A `sed` for 1 value changed other rows. | Shared value matched. | Match the unique row id. |
| A Python replace that dropped a tab broke a GDScript lambda. | Indentation lost. | Keep exact indentation; read the exact text first. |
| A no-op pair stopped the whole edit. | Old equals new. | Drop the pair and apply again. |
| A sed slip in shared data changed another unit's row. | Loose pattern. | Diff data and export against the base before READY. |

## Lead, steward and push

| Symptom | Cause | Fix |
| --- | --- | --- |
| A land brought 2 later commits with red tests. | The steward merged a branch name. | Merge the exact hash only. |
| A lead approved new gameplay elements as a "local lever". | Lead overstepped. | Gameplay items go to the owner. |
| Units built local copies of a shared fix, and the fixes conflicted. | No SHARED rule. | SHARED asks only; the lead runs shared checks once. |
| Known FAIL lines from other rows confused each worker. | No FAIL owner. | The lead owns the FAIL list; check a FAIL is the same with your old value. |
| A proposal quoted a value from an older base. | Stale base. | Diff against the stated base; if no game code changed, the table stays valid. |
| A merge brought back a held row from unit history. | Whole-branch merge. | Cherry-pick only the approved commit; revert the row in a follow-up. |
| A two-dot diff of an old branch showed newer work as removed. | Diff direction. | Check with a no-commit merge. |
| The land request named a hash, and a later handoff commit moved the tip. | Tip moved. | Tell the steward the new tip. |
| A commit landed on the wrong branch. | A worker switched the shared checkout. | Check the branch before every commit. |
| An orchestrator's `git add` went into the steward's open merge. | Shared checkout. | Check for an open merge first; take the checkout lock. |
| A keep-both resolution kept a reference to a renamed constant. | Unchecked resolution. | Parse check after every resolution. |
| An exporter re-run at land rewrote 130 lines another unit moved. | Blind regenerate. | Do not re-run; check the diff holds only the new block. |
| A branch held 44 MB of captures. | Captures committed. | A path checkout of the wanted files from the hash. |
| The land report went out before the push ended. | Early report. | Report only after the push, with the tip. |
| A push on port 22 timed out. | Network. | SSH over 443 with keepalive; an alarm around the push. |
| A hung test run blocked every worker for 25 minutes. | No time limit. | Kill it; smaller sets with a time limit. |
| A held unit had 47 failures. | Mixed owners. | Match every name against the notes; report counts per owner. |
| A stale shared temp file published another project's PR text. | Reused temp path. | A fresh unique path and a read-back. |
| A regenerate step in a fix order changed nothing. | Wrong cause. | Stop before the push and find the real cause. |
| A plan said "commit" while the fleet rules said "do not commit". | 2 rule sets. | 1 rule set per worker type. |
| A plan named a tool or file that did not exist. | Unchecked path. | Check every path before the spawn. |
| A zsh loop did not split a variable, so no worker spawned. | zsh word split. | 1 spawn command per worker. |
| Quota ran out and spawns fell back to a sandboxed agent. | Silent fallback. | Check the agent after spawn. |
| An orchestrator adopted a plan that the other fleet owned. | No cross-fleet check. | Check the pane list before you adopt a plan. |
| A peer land regenerated a data file between 2 tuning rounds. | Shared generated file. | A regenerate message; compare data per round. |
| A new signal landed without a telemetry handler. | No signal checklist. | Handler and test in the same change. |
| A sync test sim called a HUD rebuild 610 times. | UI calls in a sim. | Omit UI-only calls in sims. |
| Placeholder art captures went stale. | Final art landed. | Contact sheets now; the full capture set on final art. |

## Sandboxed workers and the fleet CLI

| Symptom | Cause | Fix |
| --- | --- | --- |
| A sandbox suite showed 100+ parse failures that hid real failures. | Sandbox limits. | The steward runs the full suite outside the sandbox. |
| A broad test filter added 15 sandbox failures. | Filter matched other folders. | Exact folder filters. |
| `ps`, `pgrep` and `nice` are blocked. | Sandbox. | Trust the exit code of a bounded run; do not guess at hung processes. |
| A runner-made `override.cfg` showed in the diff. | Sandbox runner. | Leave it out of every commit. |
| A patch tool failed 4 times on stale lines. | Old file view. | Re-read the exact lines; patch smaller pieces. |
| A fade did not show in a real window. | No window in the sandbox. | The steward checks visual work with pixel values in a real capture. |
| `note --next N` failed in about 8 sessions. | Flag not in the installed CLI. | A plain note. |
| `done --unlanded` and `done --handoff` failed. | Version drift. | Plain `done` with the reason in the text. |
| `worktree` was an unknown subcommand. | Older CLI. | Report the branch as committed and not landed. |
| `done` failed on an old-format registry file. | Version drift. | Do not edit it; tell the orchestrator. |

## Gameplay decisions and wave plans

| Symptom | Cause | Fix |
| --- | --- | --- |
| The fleet added gameplay elements on its own. | No consult rule. | Number changes go on; new or removed mechanics go to the owner. |
| A global mechanic was proposed to fix 1 hard encounter; the owner called it an over-reaction. | Global lever for a local problem. | Prefer the entity's own row. |
| A worker report pasted into an answer box looked like direction. | Mixed sources. | Only the owner's own words are direction. |
| The fleet sat idle after the owner answered. | The orchestrator waited for "continue". | Start the agreed steps at once. |
| A lead's earlier GO covered new behaviours. | GO given below the owner. | The owner's rule takes it back; send them again as proposals. |
| A spawn closed at start, and its role name stayed bound. | Registry kept the name. | A new role name with a suffix. |
| The orchestrator merged the base mid-task and a patch failed. | Stale lines. | Re-read the lines, import, patch again. |
| The context limit came before the plan index was written. | Long planning turn. | Hand off, then finish after compaction. |
| The owner check script was lost. | It lived in `/tmp`. | Keep fleet tools in the repo. |
| A rule change for a new class of content applied to every member of the family. | Rule scoped too wide. | Scope it to the new class. |
