# 08: Read the agent's working timer from its pane

**What to build:** A pure function that reads a pane's visible text and
returns how long the agent says it has been working, in seconds, or
nothing. It knows the current Codex and Claude Code formats (for example
"Working (12m 30s · esc to interrupt)" and the Claude Code spinner line
with its elapsed time). Each format is a small table entry with fixtures,
so a new release that changes the text needs 1 new entry and 1 fixture.

The watchdog (ticket 10) uses it as the last progress signal, after
transcript growth and herdr's `state_change_seq`.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

**Persona:** primary `sonnet-feature`, fallback `backend-developer`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.7 (`screen_timer_s`), 5.3 step 3, 8 item 5

- [ ] Fixtures from real panes for Codex and Claude Code, each with its
      expected seconds: under 1 min, minutes and seconds, over 1 h.
- [ ] A screen with no timer returns nothing, never 0.
- [ ] A screen with 2 timers (scrollback) returns the one nearest the
      bottom.
- [ ] The function makes no herdr call and no file read; the caller passes
      the text.
