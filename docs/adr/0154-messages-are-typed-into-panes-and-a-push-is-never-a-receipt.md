# Messages are typed into panes and a push is never a receipt

horch has no agent-to-agent protocol. `crates/horch/src/cmd/messaging.rs`
(`tell`, `inbox`, `note`, `done`) delivers a message by typing it into the
target pane's terminal and submitting it, exactly as a human who switched
panes would. Any agent CLI works unchanged. Because typing is all there is,
a successful push (text typed into a pane) never by itself confirms
receipt. `crates/horch/src/cmd/delivery_probe.rs` confirms delivery only by
a live channel relay's answer or by the orchestrator running
`horch inbox`, and every probe is bounded by a deadline.

## Consequences

- From a worker (`HORCH_ROLE` set) `messaging::tell` always starts the
  message with the worker's `[<role>]` tag. An untagged line reads as the
  human operator.
- A message over `message::MAX_INLINE` chars is written to a file. The pane
  gets 1 short line that quotes its head and names the file.
- An idle orchestrator pane can hold the operator's unsent draft, and typed
  text lands on it. So a live orchestrator relay socket always comes before
  its pane: the courier (`orchestrator_target` in `cmd/mail.rs`) prefers it,
  and `delivery_probe` never types into the pane while a relay answers.
- `messaging::try_relay` offers a line to the relay first. `None` means the
  relay took it. `Some(line)` means the caller types it, with a duplicate
  warning when the relay did not confirm. An error means the relay is live
  but did not take or confirm durable mail. The line is then not typed. It
  stays pending and the courier redelivers it through the relay.
- `delivery_probe::fleet` queues the startup probe as durable mail first. A
  live relay gets the probe and its answer confirms it. Only with no relay
  is the probe typed into the pane, and then only `horch inbox` confirms
  it. A deadline or a failed push leaves the probe durable for the normal
  courier.
- The probe is typed with herdr's atomic `agent_prompt`, which includes
  Enter. It never falls back to separate text and Enter writes in a live
  TUI: a deadline between the 2 writes could leave text attached to the
  operator's next prompt.
- `smoke::arithmetic_echo` sends unevaluated arithmetic
  (`echo SMOKE_TEST_$((40+2))`) and matches the result `SMOKE_TEST_42`.
  Matching the sent text would false-pass on a line that was pasted into
  the pane but never submitted, which is the herdr send-text failure
  `horch tell` works around.
