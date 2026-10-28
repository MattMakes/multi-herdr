# 02: horch inbox --roles, --peek, and mail printed last (P17)

**Issue:** [#48](https://github.com/MattMakes/multi-herdr/issues/48) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch-e2e --test messaging --test mail && cargo test -p horch-core --test golden_prompts --test skills_catalog`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

- **Still needed as written.** `horch inbox` still takes no flags. It prints and acknowledges mail first and the role list second, the reverse of the order this ticket wants. It has no acknowledged-count line.
- Pending mail now also includes `[horch]` task-ledger wakes, not only worker messages. The reminder that other horch commands print ("N worker messages are not acknowledged") should say "messages".

## What to build

`horch inbox | tail` must never hide messages it acknowledged. Agents always run `inbox` with piped output, so the fix does not use terminal detection.

- The role list prints first. Pending mail prints after it, and the output ends with `N messages acknowledged`.
- `--roles` prints only the role list, and neither prints nor acknowledges mail.
- `--peek` prints pending mail but does not acknowledge it.
- The orchestrator briefing and the orchestrate skill name the 2 flags where they describe `inbox`.

## Acceptance criteria

- [ ] With 3 pending messages, the last line of `horch inbox` output is `3 messages acknowledged`, and the messages come after the role list.
- [ ] `horch inbox --roles` leaves all 3 messages pending, and prints none of them.
- [ ] `horch inbox --peek` prints all 3 and leaves them pending; the courier keeps redelivering them.
- [ ] A worker running `inbox` still neither prints nor acknowledges orchestrator mail (existing behavior).
- [ ] The pending-mail reminder no longer calls every message a worker message.
- [ ] Briefing golden changes are in a named sanctioned block. The skill stays under its size budget (about 0.5 KB headroom since v0.3.0).

## Blocked by

None (can start immediately)
