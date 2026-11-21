# Mail files are enqueued in order and at most once per key, without a lock

The mail queue (`crates/horch-core/src/messaging/mail.rs`) is a set of
files that several processes write and read at once, with no lock. Each step
of enqueue, push and acknowledge is ordered so that a crash or a concurrent
reader never loses, duplicates or resurrects a mail.

## Consequences

- `mail_id` makes ids sort by `created_at`, then by the real time of the
  enqueue, because `pending` lists mail by id. 2 processes that enqueue in
  the same millisecond, or under a pinned clock (`HORCH_NOW`), keep their
  order. The pid and the counter only make the id unique (a pid as text
  would misorder, for example 10000 before 9999).
- `Mail::claim_key` writes the key file naming the mail id before the mail
  is published, so a crash at any point leaves exactly 1 id, and a replay
  publishes that same id once (1 file, 1 mail id). A key file is named by
  32 hex characters of the key's sha256, safe as a file name for any key.
- Enqueue publishes the question file before it renames the pending mail
  into place. A courier or inbox can observe and acknowledge pending mail
  as soon as that rename completes, and acknowledgement must not erase the
  question.
- `Mail::mark_pushed` records a first push with a separate marker file, not
  by rewriting the mail file. A rewrite can race `ack` and bring an
  acknowledged mail back into pending.
- Enqueue prunes acknowledged mail and expired key files best effort. A
  prune error is printed to stderr and never fails or blocks the send:
  delivering the message matters more than cleanup.
- `Id::short`, the id a mail line shows, is 8 hex characters: a 32-bit
  FNV-1a hash of the full id. Every process computes the same value from
  the file name, which a per-process randomized hasher such as Rust's
  `DefaultHasher` would not.
