# 01: Deliver orchestrator mail through the channel relay, never over the operator's typing

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-bugfix` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo test --workspace`
**Own:** `crates/horch/src/cmd/mail.rs`, `crates/horch/src/cmd/delivery_probe.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch-e2e/`, `docs/specs/`, `CHANGELOG.md`
**Do not touch:** `skills/`, `teammates/`

## Goal

horch never types mail into the orchestrator pane while the channel relay is live. The operator's half-typed prompt is never overwritten.

## Context

The operator reports this problem in 8 of 10 fleet starts. The operator starts typing to a new orchestrator. Then this line is typed into the pane over the operator's text:

```
[horch #9f59ffa5] NOTE: run horch inbox to confirm delivery (redelivery 1: not acknowledged; run horch inbox)
```

The orchestrator analysed the code. The analysis is below. Verify each point before you change code.

1. `horch fleet` calls `delivery_probe::fleet` (`crates/horch/src/cmd/recipes.rs`, near line 445). `fleet_check` in `crates/horch/src/cmd/delivery_probe.rs` enqueues the PROBE mail. Then it calls `herdr.agent_prompt(pane, PROBE)` when the pane `at_prompt()`. This types text into the orchestrator pane.
2. If nobody runs `horch inbox` in 5 s, the probe mail stays pending. The courier sends redelivery 1 at 30 s (`OrchestratorResends::push` in `crates/horch/src/cmd/mail.rs`, near line 312).
3. `Courier::deliver_due` (`mail.rs`, near line 225) types into the pane when an orchestrator pane is registered and idle. It uses the relay only when no pane is registered: `let relay_live = pane.is_none() && channel::is_live(&self.socket);` (near line 157).
4. `at_prompt()` (`crates/horch-core/src/workspace/model.rs:54`) is true while the agent waits for input. It is also true while the operator types a draft. "Idle" does not mean "the input box is empty".

## Steps

1. Reproduce. Write a failing test first. Use `crates/horch-e2e/src/bin/fake-herdr.rs` and the existing `crates/horch-e2e/tests/fleet18_delivery_probe.rs` as models. The test sets up an orchestrator pane that is at its prompt and a live channel relay. It asserts that the courier sends the redelivery through the relay and sends no pane text. Check: the test fails on `main`.
2. Change `Courier::deliver_due` and the relay decision in `mail.rs`. When the relay is live, send orchestrator mail through `channel::send` only. Type into the pane only when the relay is not live. Check: the step 1 test passes.
3. Change `fleet_check` in `delivery_probe.rs`. When the relay is live, send the probe through the relay, not `agent_prompt`. Type into the pane only when the relay is not live. Check: add a test that asserts no `agent_prompt` call when the relay is live.
4. Decide whether the probe still needs `horch inbox` from the orchestrator. Today the redelivery repeats a NOTE that has no value to the operator. Preferred option: a channel send that succeeds confirms delivery, and the PROBE mail is acknowledged without a redelivery. If you keep the ack, the probe redelivery must go through the relay only. Write your choice and reason in the DONE summary.
5. Keep worker-pane delivery unchanged. Worker panes have no relay. Check: `a_worker_pane_keeps_its_per_message_redelivery` and every other `mail.rs` test still pass.
6. Update the delivery text in `docs/specs/` that describes orchestrator mail. Find it with `grep -rln "redelivery" docs/specs`. State: relay first, pane text only when the relay is down.
7. Add 1 line under the unreleased section of `CHANGELOG.md`. Example: "horch no longer types mail over your prompt when the channel relay is live."
8. Run `cargo test --workspace`. Check: it exits 0.

## Constraints

- Do not use `ANTHROPIC_API_KEY`. Run any `claude` command as `env -u ANTHROPIC_API_KEY claude ...`.
- Do not add a retry or a sleep to hide a race. Fix the cause.
- Do not change the mail file format or the mail id format.
- Do not change the relay-down fallback. With no relay, pane text is still the only path.
- Out of scope: worker delivery, the digest logic, and the skills under `skills/`.

## Done when

- A new test proves that a live relay plus an idle orchestrator pane gives a relay send and 0 pane writes, for both the courier and the startup probe.
- `cargo test --workspace` exits 0.
- The spec text and `CHANGELOG.md` describe the new behavior.

## Report

Record a progress note after each step. Send the DONE summary with `horch ledger done`. Name the test files, the probe decision from step 4, and every test you changed.
