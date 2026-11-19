# Ticket format (contract C1)

Producer: the sub-orchestrator (`idea-lead`) in its ticketing phase, and the ticket-writer
(`teammates/ticket-writer.md`). Consumer: the plan validator of fleetd, and `horch ledger add-map
--dry-run` (`crates/horch-core/src/taskledger/map.rs`). Decisions D2, D5, D12 and D14 of
`docs/specs/trace-fixes.md` set this contract. `trf_02_*` tests pin the ticket-writer's examples to
it.

Specs live in `docs/specs/<slug>.md`. Tickets and their map live in `docs/tickets/<slug>/`. Both are
committed. `plan.yaml` (section 4) names the tickets of an idea and replaces `MAP.md` for a new idea.

## 1. Ticket file

A ticket is 1 file, `docs/tickets/<slug>/<NN>-<name>.md`, numbered from `01` in dependency order.

### 1.1 Field block

The block is the first lines after the `# NN: Title` heading. One field per line, in this order.
Unknown fields are an error under `add-map --dry-run`. Values in backticks are literal.

```
<!-- horch-ticket: 1 -->
**Persona:** primary `<teammate>`, fallback `<teammate>` · **Parallel:** yes|no
**Kind:** feature|quality|refactor|security|docs|report
**Review:** none|light|full
**Check:** `<command>`            (or the word none, meaning "this ticket has no check")
**Own:** `<path>`, `<dir/>`        (omit only for report tickets)
**Do not touch:** `<path>`         (optional)
```

- `horch-ticket` is the format version. The parser refuses a version it does not know, instead of
  misreading it.
- `Persona`: the primary is required. The fallback is optional. `Parallel` defaults to `yes`.
- `Kind` sets the default `Review`: `feature`, `docs` and `report` give `none`. `quality`,
  `refactor` and `security` give `full`. An explicit `Review` wins.
- `Review`: `none` has no code review. `light` is 1 round with at most 2 reviewers. `full` is at
  most 3 rounds with at most 3 reviewers per round.
- `Check`: the command that proves the ticket. horch runs it before it lands the work. When the
  line is absent, the ticket inherits the run's default check. Write `**Check:** none` to run no
  check.
- `Own`: repo-relative files and directories the worker may edit. A directory ends with `/`.
- `Do not touch`: optional paths the worker must not edit.

### 1.2 Body

After the field block: `## Goal`, `## Requirements` (when the spec has requirement ids),
`## Steps` and `## Done when`. The body names real paths and real commands.

### 1.3 Example ticket

```markdown
# 01: Read the retry limit from the config file

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p app-core -- retry_limit`
**Own:** `crates/app-core/src/config.rs`, `crates/app-core/tests/retry_limit.rs`
**Do not touch:** `crates/app-core/src/net/`

Spec: `docs/specs/retry-limit.md` (section 3).

## Goal

A user sets `retry_limit` in `app.toml`, and every request retries at most
that many times.

## Requirements

- **RL-01**: `retry_limit` in `app.toml` sets the limit. The default is 3.

## Steps

1. Write the test `retry_limit_from_file` first.
2. Read `retry_limit` in the config loader. Refuse a negative value.

## Done when

The Check passes, and a negative value gives an error that names the key.
```

## 2. Map file

`docs/tickets/<slug>/MAP.md` schedules the tickets of the old flow. A new idea uses `plan.yaml`
(section 4). A `MAP.md` stays readable: the run in flight at the update to Phase 6 keeps its
`MAP.md`, and `horch ledger add-map --dry-run <MAP.md>` still checks one.

- A `Spec:` line names the spec path.
- The first table has 1 row per ticket and the columns `#`, `Title`, `Blocked by` and `Wave`. The
  title links the ticket file by its name relative to the map.
- `Blocked by` holds ticket keys of the same map (`01`, `1`) or `T-<n>` tasks already in the run.
  An empty cell means no blocker.
- The map has no `Areas` column. The persona, the check and the owned paths live in the ticket.
  A `Persona` or `Parallel` column, when present, wins over the ticket's line.
- Wave 1 is every ticket with no blockers. Wave N is every ticket whose blockers are all in
  earlier waves.
- Below the table: `## Waves`, `## Critical path` and `## Open questions`.

### 2.1 Example map

```markdown
# Map: retry limit

Spec: `docs/specs/retry-limit.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [Read the retry limit from the config file](01-retry-limit-config.md) |  | 1 |
| 02 | [Apply the retry limit to every request](02-retry-limit-requests.md) | 01 | 2 |

## Waves

- Wave 1: 01
- Wave 2: 02

## Critical path

01 -> 02

## Open questions

None.
```

## 3. What `add-map` does with a ticket

Ticket 10 of `docs/tickets/trace-fixes/` implements the version check, the `Kind` and `Review`
fields, the snapshot and `--dry-run`. Until it lands, `add-map` reads `Persona`, `Parallel`,
`Check`, `Own` and `Do not touch`.

- It queues 1 task per map row, with the ticket's personas, `Check`, `Own` and `Do not touch`.
- It stores the ticket text and its sha256 on the task (`ticket_snapshot`, `ticket_sha`). `claim`
  prints the snapshot, then the path.
- `add-map --dry-run` parses the map and every ticket, and reports each error without a change.

`horch ledger add-map` without `--dry-run` is retired: it exits 1 and names the new command. The
sub-orchestrator writes `plan.yaml` and its tickets, and fleetd validates and commits the plan
after your approval. For 1 ticket that you wrote, run `horch idea add --direct <ticket file>`.

## 4. plan.yaml

`plan.yaml` replaces `MAP.md` for a new idea. The sub-orchestrator writes it in the ticketing phase,
in the idea folder `ideas/I-NNNN/`. A ticket stays a Markdown file in format C1 (section 1), and
`plan.yaml` names it with the `ticket` field of its task. The plan validator checks the schema of
each task, each ticket that the plan names, the DAG of `after`, and that each requirement of
`requirements.yaml` has a task (`covers`). fleetd commits a valid plan and its tasks in 1
transaction. `horch ledger add-map --dry-run <plan.yaml>` runs the same validator and writes
nothing.

### 4.1 Example plan

The example of `docs/specs/fleetd/design.md` (Workspace layout and file protocol):

```yaml
idea: I-0007
repo: /fleet/repos/app
feature: F-0003
base_branch: feature/retry-sync-backoff   # the feature branch; every task of the plan lands here
tasks:
  - key: backoff
    title: Add exponential backoff to SyncClient.request
    ticket: tickets/01-backoff.md   # Markdown, today's format C1
    kind: feature
    profile: coder-codex
    covers: [R1]
    inputs: [src/sync/client.py]
    outputs: [src/sync/client.py, tests/test_backoff.py]
    acceptance:
      - run: pytest tests/test_backoff.py -q
      - run: pytest tests/sync -q
    budget: {max_minutes: 40}
  - key: logging
    title: Log each retry with attempt and delay
    kind: feature
    profile: coder-claude
    covers: [R2]
    after: [backoff]        # met when backoff is done and its task branch is landed on the feature branch
    acceptance:
      - run: pytest tests/test_backoff.py -q -k logs
  - key: sync-suite
    title: Run the existing sync suite on the feature branch
    kind: quality
    profile: coder-codex
    covers: [R3]
    after: [backoff, logging]
    acceptance:
      - run: pytest -q
  - key: retry-doc
    title: Document the retry behavior in docs/sync.md
    kind: docs              # a judgment kind: structural check plus a judge
    profile: writer-claude
    covers: [R2]
    outputs: [docs/sync.md]
    acceptance:
      - sections: [docs/sync.md, "## Retries"]   # structural: the file and the section exist
  - key: retry-doc-judge
    title: Judge docs/sync.md against R1 and R2
    kind: judge             # not a judgment kind: it needs no judge of its own
    judges: retry-doc       # plan validation refuses a judgment task with no judge; met when retry-doc is in verifying and its structural check passed
    profile: judge-codex    # a different session, and a different provider from the author
                            # no after: [retry-doc]; the worktree starts from the tip of the retry-doc task branch, and retry-doc is done only after this verdict
    outputs: [docs/sync.review.md]   # each miss ranked CRITICAL, HIGH, MEDIUM or LOW
```
