# Ticket format (contract C1)

Producer: the ticket-writer (`teammates/ticket-writer.md`). Consumer: `horch ledger add-map`
(`crates/horch-core/src/taskledger/map.rs`). Decisions D2, D5, D12 and D14 of
`docs/specs/trace-fixes.md` set this contract. `trf_02_*` tests pin the ticket-writer's examples to
it.

Specs live in `docs/specs/<slug>.md`. Tickets and their map live in `docs/tickets/<slug>/`. Both are
committed.

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

`docs/tickets/<slug>/MAP.md` schedules the tickets.

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
