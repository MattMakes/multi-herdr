# Orchestrator mail goes through the channel relay while it is live

Mail to the orchestrator goes through the Claude channel relay whenever a
relay answers. horch types into the orchestrator pane only when no relay
answers. An idle orchestrator pane can hold the operator's half-typed
prompt, and typed text would corrupt it. In the incident, the pane was
idle because the operator was typing a draft while the relay was live. The
courier's redelivery now goes through the relay and nothing is typed into
the pane (`crates/horch-e2e/tests/relay_first_delivery.rs`).

## Consequences

- A refused or unconfirmed relay send leaves the mail pending for the
  courier. It does not fall back to typing.
- The socket file appears at bind, before the relay listens, and a connect
  in that gap is refused. A relay is ready only when it answers a ping
  (`channel::is_live`), not when its socket file exists.
