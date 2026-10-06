---
name: data-migrations
description: Use when a change alters persisted data - a schema change, a new or dropped column or index, a type change, a backfill, a data fix, a file or message format change - to make it safe against existing rows and live traffic. Covers expand/contract order, online-safe DDL, batched backfills, old and new code running side by side, a rollback path and a verified result.
---

# Data migrations

Change persisted data without downtime, without loss, and with a way back. Code is easy to roll back; data with live readers is not. Every migration must be safe to run against the rows that exist today, while the old code is still serving, and safe to stop halfway.

Use the migration tool the repository already uses (Rails, Django, Alembic, Flyway, Liquibase, Prisma, golang-migrate, sqlx, EF Core, Diesel, or plain SQL files). Check its exact behaviour (transactions, locking, ordering) with the docs tool you have; do not recall it.

## When to use

- A schema change: table, column, index, constraint, type, default, enum value.
- A backfill or a one-off data fix.
- A change to a stored file format, a cache value shape, a serialized message or an event schema.
- Not for: a brand-new table that no code reads yet, in a single additive migration. That is safe; write it and move on.

## The expand/contract order

Never change data shape and the code that reads it in one step. Split the change into deploys, each one safe alone and safe to roll back:

1. **Expand.** Add the new shape beside the old one: a new nullable column, a new table, a new index (built online), a new enum value. Old code ignores it.
2. **Dual write.** Deploy code that writes both the old and the new shape. Reads still use the old one.
3. **Backfill.** Copy existing rows into the new shape in batches (see below). Verify.
4. **Switch reads.** Deploy code that reads the new shape. Keep writing both, so a rollback of this deploy is safe.
5. **Stop old writes.** Deploy code that writes only the new shape.
6. **Contract.** After a full release cycle with no reads of the old shape, drop it in its own migration.

Small changes can merge steps, but only when you can show that old and new code running side by side is correct at every point. A rename is never one step: it is add, dual write, backfill, switch, drop.

## Workflow

1. **Read the current state.** Find the real schema (the latest migration, or the live schema dump the repo keeps), the models, and every reader and writer of the data you will change: application code, background jobs, reports, analytics exports, other services. Grep for the table and column names, including in raw SQL strings. Check: you have a list of readers and writers with paths.

2. **Size the data.** Find or estimate the row count and the write rate of each affected table. Say where the number comes from. A migration that is instant on a dev database can lock a large production table for minutes. If you cannot find the size, say so, and design for a large table.

3. **Choose the steps.** Write the expand/contract steps for this change as a numbered list. For each step: the migration or the code change, why it is safe with the previous code still running, and how to roll it back.

4. **Make each DDL statement online-safe.** Check every statement against the database engine's locking behaviour. Common rules (verify them for the engine and version in use):
   - Add a column as nullable, or with a constant default the engine can add without rewriting the table. Add `NOT NULL` later, after the backfill, using the engine's safe method (for example, a `CHECK ... NOT VALID` constraint validated separately, on PostgreSQL).
   - Build indexes online (`CREATE INDEX CONCURRENTLY` on PostgreSQL, `ALGORITHM=INPLACE, LOCK=NONE` on MySQL where supported). A concurrent build cannot run inside a transaction; the migration tool may need a flag for that.
   - Add foreign keys and check constraints without a full validation lock where the engine allows (`NOT VALID`, then `VALIDATE CONSTRAINT`).
   - Do not change a column type in place on a large table. Add a new column and migrate.
   - Set a lock timeout for the migration session, so a blocked DDL fails fast instead of queueing all traffic behind it.
   - SQLite's `ALTER TABLE` can only rename a table or column, add a column and drop a column. For any other change (a type, a constraint, a default), follow SQLite's documented 12-step table rebuild, inside a transaction, with foreign keys handled.
   Check: each statement has a note on what it locks and for how long.

5. **Write the backfill as a batched, resumable job.** Not as one `UPDATE` over the whole table.
   - Process rows in key order, in batches small enough to finish in well under a second each (start with about 1,000 rows and measure).
   - Commit each batch. Record progress (the last key done) so that a restart continues.
   - Make it idempotent: running it twice gives the same result. Only touch rows that still need it (`WHERE new_col IS NULL`).
   - Throttle between batches, and stop when replica lag or error rate rises, if the repo has a way to see them.
   - Keep it out of the schema migration when it is large, so a slow backfill does not block deploys. Run it as a job or a script with a dry-run mode that prints the count it would change.
   Check: the dry run prints the row count, and a second real run changes 0 rows.

6. **Handle the rows that do not fit.** Real data has nulls, duplicates, bad encodings, orphans and values outside the new type. Query for them before you write the backfill. Decide for each case: convert, default, quarantine to a side table, or stop and ask. Write the decision down. Send a `QUESTION:` to the orchestrator for a case where the choice loses or changes user data.

7. **Plan the rollback.** For each step, write what rolling back means:
   - Expand steps roll back by deploying the previous code; the extra column or table stays and is harmless.
   - A `down` migration that drops a column with new data in it destroys data. Say so, and prefer leaving it.
   - Backfills: keep the old values until the contract step, so the switch of reads can be undone.
   - A destructive step (drop, truncate, irreversible transform) needs a backup or export that has been checked to restore, and goes last.
   Check: there is no point in the order where a rollback loses data written by users.

8. **Test against realistic data.** Run the migrations up from the current schema on a database with representative rows, including the bad rows from step 6. Run the test suite with the old code against the expanded schema, and the new code against it. Run `down` where the tool supports it, then `up` again. Where the repo has migration tests, add one. Use `tdd` for behaviour changes in the code.

9. **Verify the result.** After the backfill, compare: row counts, a checksum or a sample of old against new values, the count of rows still unconverted (it should be 0), and the constraint validation. Write the queries into the PR or the runbook so that whoever runs the migration in production can repeat them.

10. **Report.** In `DONE:` give the steps and which deploy each belongs to, the lock notes, the backfill command and its dry-run output, the verification queries, and the rollback path. If a step must wait for a release cycle (the contract step), say so; do not do it in the same change.

## Non-database data

The same order applies to formats that live outside a database:
- **Serialized files, cache values, messages, events:** readers first. Deploy readers that accept both the old and the new format, then writers that produce the new one, then remove the old reader after the old data has expired or been rewritten. Put a version field in the format if it has none.
- **Caches:** change the key (add a version) rather than the value shape under the same key, so old and new code do not read each other's entries.
- **Queues and topics:** old messages already in flight are read by new code. New messages can be read by old code during a rollout or a rollback. Both must work.

## Review checklist

- [ ] All readers and writers of the changed data are listed, with paths.
- [ ] The table sizes are stated, with sources, or the design assumes large.
- [ ] The change is split into expand/contract steps, each safe alone and reversible.
- [ ] Each DDL statement has a lock note; a lock timeout is set.
- [ ] The backfill is batched, resumable, idempotent and has a dry run.
- [ ] Bad rows were queried, and each case has a written decision.
- [ ] No rollback point loses user-written data; destructive steps go last, with a checked backup.
- [ ] Verification queries are written down and were run on test data.
