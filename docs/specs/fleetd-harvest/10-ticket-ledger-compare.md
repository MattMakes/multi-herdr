# 10: Compare the ticket folder with the ledger at run close

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: FM-1. Size: S.

Path names in this spec: `TL` is `crates/horch-core/src/taskledger`. `CMD` is
`crates/horch/src/cmd/taskledger.rs`. Labels: `CODE`, `MEASURED`, `REPORT`, `NOT VERIFIED`.

## Problem

The gap is real in the code of MAIN. The ledger shows 0 incidents of it in the runs that can be
measured (see `## Evidence on main`). This fix is a cheap guard. It does not repair a measured loss.

The orchestrator writes tickets into a folder, and then queues them. If it does not queue 1 ticket,
no part of horch notices. The other tasks finish, the run gate passes, and the run closes as `done`.
The operator reads "closed as done", and a part of the idea is never built.

The exact cause has 2 parts:

1. A task exists only when an agent calls `add` or `add-map`. `CODE TL/tasks.rs:315` (`add`),
   `CODE TL/tasks.rs:349` (`add_many`), `CODE CMD:2252` (`add_map`). No code reads a ticket folder
   to find a ticket that the map does not name.
2. `close_inner` looks only at rows that exist. It refuses on open tasks, open worktrees, parked
   worktrees, a missing gate and an unmerged PR. `CODE TL/runs.rs:490-533`, `CODE TL/runs.rs:830`.
   The condition of the fault: a ticket file has no row in `tasks`, so `status::unsettled` cannot
   return it (`CODE TL/status.rs:303`), and `close` sets the run to `done` (`CODE TL/runs.rs:539`).

`add-map` is 1 transaction (`CODE CMD:2264`), so a map row is not lost halfway. The fault needs 1 of
these: the map omits a ticket, a ticket is written after `add-map`, or a 2nd map is not queued.

## Evidence on main

R6 has no count: `REPORT research/06-motive-and-journey.md:40` and `:248`. The operator named the
problem in issue #349. The ledger has no event for it. I therefore measured the file state directly.

Measurement date: 2026-11-22T23:14Z. The ledger starts on 2026-10-31. Save this script outside the
repository and run `python3 -I measure.py "$(horch ledger path)"`. It opens the ledger read-only.

```python
import sqlite3, os, re, collections, sys
db = sqlite3.connect(f'file:{sys.argv[1]}?mode=ro', uri=True)
def is_ticket(p):
    n = os.path.basename(p)
    if not n.endswith('.md') or n.startswith('MAP') or not os.path.isfile(p): return False
    t = open(p, errors='ignore').read()
    return bool(re.search(r'<!--\s*horch-ticket:', t)) or bool(re.search(r'\*\*persona:\*\*.*primary', t, re.I))
known = collections.defaultdict(set)
for pid, root, f in db.execute("select r.project_id,p.root,t.task_file from tasks t join runs r using(run_id) join projects p using(project_id) where t.task_file is not null"):
    known[pid].add(os.path.realpath(f if os.path.isabs(f) else os.path.join(root, f)))
s = collections.Counter(); refuse = []
for run_id, pid, slug, status, root in db.execute("select r.run_id,r.project_id,r.slug,r.status,p.root from runs r join projects p using(project_id) where r.slug not like 'solo-%'").fetchall():
    s['runs'] += 1; dirs = set(); tb = os.path.realpath(os.path.join(root, 'docs/tickets'))
    c = os.path.join(tb, slug)
    if os.path.isdir(c): dirs.add(c)
    for (f,) in db.execute("select task_file from tasks where run_id=? and task_file like '%.md'", (run_id,)):
        d = os.path.dirname(os.path.realpath(f if os.path.isabs(f) else os.path.join(root, f)))
        if os.path.dirname(d) == tb or os.path.isfile(os.path.join(d, 'MAP.md')): dirs.add(d)
    dirs = {d for d in dirs if os.path.isdir(d)}
    if not dirs: s['runs_no_folder'] += 1; continue
    s['runs_with_folder'] += 1; miss = []
    for d in sorted(dirs):
        for n in sorted(os.listdir(d)):
            p = os.path.join(d, n)
            if is_ticket(p):
                s['tickets'] += 1
                if os.path.realpath(p) not in known[pid]: miss.append(os.path.relpath(p, root))
    s['missing'] += len(miss)
    if miss: refuse.append((slug, status, miss))
print(dict(s)); print('runs refused:', refuse)
```

Results, all `MEASURED`:

1. The ledger has 42 runs: 21 solo runs and 21 other runs. Of the 21 other runs, 14 have a ticket
   folder and 7 have none.
2. The script compares 412 tickets, counted once for each run that reads the folder. 0 tickets have
   no task. 0 runs are refused. The fix therefore blocks no existing run.
3. A match inside the same run only is wrong. It refuses 3 runs: `godot` (3 tickets), `skill-v2`
   (48) and `skill-v2b` (1). Their tickets have tasks in another run of the project.
4. `docs/tickets/` holds 21 folders and 302 tickets across all projects. 12 tickets have no task in
   any run: 4 in `quiet-orchestrator-mail` and 8 in `video-formats`. Commit `05ee2342` (2026-11-10)
   added both folders.
5. Of 122 ticket files with a stored sha256, 120 match the file on disk. 2 differ:
   `going-ham-playable` T-5 and T-7. Both tasks are `cancelled`.

The 12 tickets of result 4 are the nearest real examples. They have no task id and no worker. They
are not proof of lost work: MAIN has `try_network` for `quiet-orchestrator-mail` ticket 03
(`CODE TL/git.rs:44`), and 15 skill or teammate files name `meta.format` for `video-formats`.
Whether each of the 12 was built is `NOT VERIFIED`.

Limits of the count: the script reads the files on disk today, not the files at each close or on
run-branch tips. The 7 runs with no folder cannot be measured.

## How fleetd solves it

fleetd removes the agent from task creation. `validate_plan` reads `plan.yaml`, validates it and
creates every task row in 1 transaction (`CODE 9bc3bf70:crates/horch-core/src/fleetd/ideas/application/plan_commit.rs:107`).
`plan_rules_coverage::check` refuses a plan when a requirement has no task
(`CODE 9bc3bf70:crates/horch-core/src/fleetd/ideas/domain/plan_rules_coverage.rs:15`).

The rule in 1 sentence: no work is complete unless each declared unit of work has a task.

1. The idea: compare what was declared with what the ledger holds, in code, and refuse a gap.
2. The daemon part: the idea state machine, `plan.yaml`, the verdict file and the idea folder. Do
   not port them.
3. fleetd does not compare the folder with the plan. `read_plan` reads only the tickets that
   `plan.yaml` names (`CODE plan_commit.rs:150`), so an extra ticket file is ignored. This fix
   covers that case, and fleetd does not.

## Fix

Requirements:

1. **H10-R1, run folders.** The ticket folders of a run are `docs/tickets/<slug>/`, plus the parent
   directory of each task file of the run when that directory is a direct child of `docs/tickets/`
   or holds a `MAP.md`. The second rule finds a folder whose name is not the run slug, and it
   ignores a brief in a temporary directory.
2. **H10-R2, ticket files.** A ticket is a direct-child `*.md` file whose name does not start with
   `MAP`, and for which `map::ticket_facts` returns a `version` or a `primary`
   (`CODE TL/map.rs:223`). horch reads the folder on disk at the project root and at the run-branch
   tip, and joins the 2 lists.
3. **H10-R3, match.** A ticket has a task when a task of any run of the same project, in any status,
   has the same repo-relative path, or has a `ticket_sha` equal to the sha256 of the ticket text.
   The sha match keeps a renamed ticket matched. Before the compare, remove an absolute project-root
   prefix and a leading `.worktrees/<name>/`. A cancelled task counts, because a cancel is a
   recorded decision.
4. **H10-R4, refuse.** `horch ledger run close` without `--abandon` refuses when a ticket has no
   task, and changes nothing. This check runs before the gate check. The error is:
   `run <slug> cannot close: <n> ticket(s) have no task: <paths>. Queue each with horch ledger add --file <path> --primary <persona>, or run horch ledger run close --run <slug> --leave-out <path>="<reason>"`.
5. **H10-R5, accept a ticket.** `--leave-out <path>=<reason>` can repeat. Each value accepts 1
   ticket. horch refuses a path that is not in the missing list, and an empty reason. On a close
   that succeeds, horch records 1 `note` event for each value, with the text
   `ticket left out: <path>: <reason>`. A close that fails rolls these events back.
6. **H10-R6, runs with no folder.** The check does not apply to a solo run (`runs::is_solo`,
   `CODE TL/runs.rs:228`) or to `--abandon`. A one-off task has the file name
   `(one-off: no ticket file)` (`CODE TL/tasks.rs:21`), so it adds no folder. A planned run with no
   folder passes.
7. **H10-R7, early warning.** `horch ledger run gate` prints 1 line for each missing ticket:
   `NOTE: ticket <path> has no task.` The exit code of the gate does not change. Reason: the
   operator merges the PR after the gate and before the close, so the close alone warns too late.
8. **H10-R8, read errors.** An error other than "not found" on a ticket folder refuses the close and
   names the path. A file that is not UTF-8 is not a ticket.
9. **H10-R9, changed ticket.** At close, horch prints
   `NOTE: ticket <path> changed after T-<n> was queued.` when the newest task of that path is not
   cancelled, has a `ticket_sha`, and the sha differs from the file. This note never refuses.

Files of MAIN to change:

1. New `TL/ticket_compare.rs`: `read_run_tickets(conn, git, run, tip)` for H10-R1, R2 and R8, and
   `compare(conn, run, &tickets)`, which returns the missing and changed tickets (H10-R3, R9).
2. `TL/mod.rs:1-21`: add `pub mod ticket_compare;`.
3. `TL/runs.rs:824`: add `tickets` and `left_out` to `CloseGuards`. `TL/runs.rs:830`: call `compare`
   first in `check_guards`, refuse, and write the events. `TL/runs.rs:946`: update the test helper.
4. `CMD:315`: add `--leave-out` to `RunCommand::Close`, with `conflicts_with = "abandon"`.
   `CMD:1696`, `CMD:1707`, `CMD:1747`: pass the values, and read the tickets in `close_guards`,
   outside the write transaction. `CMD:1765`: print the H10-R7 notes in `gate_cmd`.
5. `teammates/_base/fleet-orchestrator.md:143` and its pin `crates/horch-core/tests/golden_prompts.rs:1607`:
   add the new refusal and `--leave-out` to step 3 of "Close the run". Change both in 1 commit.
6. `skills/orchestrate/SKILL.md:21`: add `--leave-out` to the flags of the close command.
7. New test file `crates/horch-e2e/tests/h10_ticket_compare.rs`.

New settings: none. The check has no off switch, because `--leave-out` is the narrow exception.

Not ported: `plan.yaml`, the plan validator, the requirement coverage rule, the idea state machine
and the daemon. Requirement coverage already has `scripts/check-req-coverage.sh`.

Effect at upgrade:

1. Ledger: no schema change and no migration. The fix reads existing columns.
2. Existing run: the check applies at its next close. The measurement refuses 0 of 14 runs.
3. Live worker: no effect. Workers do not call `run close`.
4. Live orchestrator: its prompt lacks `--leave-out` until it restarts. The refusal prints the flag.

## Reproduce

Automated (`NOT RUN`, because the limits of this run forbid `cargo`). Crate: `horch-e2e`. File:
`crates/horch-e2e/tests/h10_ticket_compare.rs`. Copy `text`, `world` and `ok` from
`crates/horch-e2e/tests/i308_triage.rs:9-37`. The map shape comes from
`crates/horch-e2e/tests/taskledger.rs:301`.

```rust
#[test]
fn h10_r4_close_refuses_a_ticket_with_no_task() {
    let h = world("h10-r4");
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let dir = h.project.join("docs/tickets/alpha");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, title) in [("01-a", "01: A"), ("02-b", "02: B")] {
        let body = format!("# {title}\n\n**Persona:** primary `opus` · **Parallel:** yes\n");
        std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
    }
    std::fs::write(
        dir.join("MAP.md"),
        "| # | Title | Blocked by |\n|---|---|---|\n| [01](01-a.md) | A | - |\n",
    )
    .unwrap();
    ok(&h, &["ledger", "add-map", "docs/tickets/alpha/MAP.md"]);
    ok(&h, &["ledger", "cancel", "T-1", "not needed now"]);
    ok(&h, &["ledger", "run", "gate", "--check", "true"]);
    let o = h.run(&["ledger", "run", "close"]);
    assert!(!o.status.success(), "{}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("docs/tickets/alpha/02-b.md"), "{err}");
}
```

Expected on MAIN, from the code: `run close` exits 0 and prints `Run alpha closed as done.`
(`CODE CMD:1743`). The first `assert!` fails.

Manual (all steps `NOT RUN`). Warning: use a plain shell outside a fleet pane, and set the scratch
ledger first. Without it, these commands write to the live ledger.

1. `NOT RUN`. `export HORCH_LEDGER_DB=/tmp/h10/ledger.sqlite3 HORCH_NO_WAKE=1 HORCH_NO_WATCHDOG=1`.
2. `NOT RUN`. `mkdir -p /tmp/h10/repo/docs/tickets/demo`, then `cd /tmp/h10/repo`, then
   `git init -q -b main`, then `git commit -q --allow-empty -m init`.
3. `NOT RUN`. `horch ledger run new demo`. Expect the run and the branch `fleet/demo`.
4. `NOT RUN`. Write `01-a.md`, `02-b.md` and a `MAP.md` that lists only `01`, as the test does.
5. `NOT RUN`. `horch ledger add-map docs/tickets/demo/MAP.md`. Expect `T-1 queued for opus: A.` and
   `1 tasks queued from <path>.`
6. `NOT RUN`. `horch ledger cancel T-1 "demo"`.
7. `NOT RUN`. `horch ledger run gate --check true`. Expect `Run gate passed on <sha>. Log: <path>.`
   MAIN prints no line about `02-b.md`.
8. `NOT RUN`. `horch ledger run close`. MAIN prints `Run demo closed as done.` This is the fault.
9. `NOT RUN`. `sqlite3 -readonly "$HORCH_LEDGER_DB" "select task_file from tasks"`. Expect only
   `docs/tickets/demo/01-a.md`.

## Validate

The test of `## Reproduce` must pass after the fix. Add 1 test for each requirement in the same file.
All are `NOT RUN`. "MAIN today" is the result that the code of MAIN gives.

1. `h10_r1_finds_a_folder_from_a_task_file`: the folder is `docs/tickets/other/` with a `MAP.md`, and
   the run slug is `alpha`. Expect a refusal. MAIN today: closes, so the test fails.
2. `h10_r2_reads_a_ticket_only_on_the_run_branch`: commit `02-b.md` only on `fleet/alpha`. Expect a
   refusal. MAIN today: closes, so the test fails.
3. `h10_r2_ignores_notes_and_maps`: the folder also holds `notes.md` and `MAP-old.md`. Expect a
   close. MAIN today: closes. This test is a guard against a false refusal, and it proves no fix.
4. `h10_r3_counts_a_task_of_another_run`: an earlier run of the project queued the ticket. Expect a
   close. MAIN today: closes. This test is a guard only.
5. `h10_r3_counts_a_renamed_ticket_by_sha`: rename a queued ticket and keep its text. Expect a close.
   MAIN today: closes. This test is a guard only.
6. `h10_r4_close_refuses_a_ticket_with_no_task`: MAIN today: closes, so the test fails.
7. `h10_r5_leave_out_closes_and_records_the_reason`: expect a close and 1 `note` event. MAIN today:
   clap refuses the unknown flag `--leave-out`, so the test fails.
8. `h10_r5_leave_out_refuses_a_wrong_path_and_an_empty_reason`: expect 2 refusals and 0 events. MAIN
   today: clap refuses the flag. The refusal has the wrong cause, so assert the error text.
9. `h10_r6_abandon_and_solo_skip_the_check`: expect `--abandon` to close, and `read_run_tickets` to
   return an empty list for a solo run. MAIN today: `--abandon` closes (guard only), and the function
   does not exist.
10. `h10_r7_gate_prints_the_note_and_keeps_its_exit_code`: expect exit 0 and the `NOTE:` line. MAIN
    today: exit 0 and no line, so the test fails.
11. `h10_r8_unreadable_folder_refuses`: `chmod 000` the folder, and restore it at the end of the
    test. Expect a refusal that names the path. MAIN today: closes, so the test fails.
12. `h10_r9_changed_ticket_prints_a_note`: edit a queued ticket after `add-map`. Expect a close and
    the `NOTE:` line. MAIN today: closes with no line, so the test fails.

Manual steps with the new output (`NOT RUN`). Repeat steps 1 to 6 of `## Reproduce`, then:

1. `NOT RUN`. `horch ledger run gate --check true`. Expect `NOTE: ticket docs/tickets/demo/02-b.md has no task.`
   and exit 0. MAIN today: no note.
2. `NOT RUN`. `horch ledger run close`. Expect a non-zero exit and the H10-R4 error. MAIN today:
   `Run demo closed as done.`
3. `NOT RUN`. `horch ledger run close --leave-out docs/tickets/demo/02-b.md="out of scope"`. Expect
   `Run demo closed as done.` MAIN today: clap refuses the flag.
4. `NOT RUN`. `sqlite3 -readonly "$HORCH_LEDGER_DB" "select text from events where kind='note' and text like 'ticket left out:%'"`.
   Expect 1 row. MAIN today: 0 rows.

Measure again 2 weeks after the fix lands:

1. Run the script of `## Evidence on main`. Target: `missing` stays 0.
2. Count the accepted tickets:
   `sqlite3 -readonly "$(horch ledger path)" "select count(*) from events where kind='note' and text like 'ticket left out:%'"`.
   Baseline on MAIN: 0, because no code writes this text. Read each reason to find a bypass.

## Size and risks

Estimate: 1 worker-day. The change is 1 small module, 2 guarded call sites and 12 tests.

Risks:

1. RISK-1: an orchestrator passes `--leave-out` to get past the refusal. Mitigation: each ticket
   needs its own path and reason, and the ledger keeps both.
2. RISK-2: the close refuses after the PR is merged. Queuing the ticket then needs new work on a
   merged run branch. Mitigation: H10-R7 warns at the gate, before the merge. The path of the new
   work after a merge is `NOT VERIFIED`.
3. RISK-3: a folder that no run links is not checked. This covers the 12 tickets of evidence result
   4, because their folder name is no run slug and no task names a file in it. See Q1.
4. RISK-4: `read_run_tickets` runs outside the write transaction, so a ticket written between the
   read and the close is missed.

Files that another spec of this run can also change: `TL/runs.rs` and `CMD` near the run gate. The
ticket of spec 09 (verdict tied to commit) cites the run gate rule. Whether spec 09 edits
`check_guards` or `gate_cmd` is `NOT VERIFIED`, because that spec is not on my base commit.

Open questions for the operator:

1. Q1: Should horch also report a `docs/tickets/` folder that no run links? Recommendation: no. The
   12 tickets of that kind show no proven loss, and the report has no run to attach to.
2. Q2: Should a changed ticket (H10-R9) refuse the close? Recommendation: no. An edit after queueing
   can be a typo fix, and the measured count of live cases is 0.
