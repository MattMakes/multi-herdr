# Codex teammates name a model family

A Codex teammate names `sol`, `astra`, `terra` or `luna` in its `model:`
field. horch resolves that family to the highest numeric version whose Codex
catalog visibility is `list`. A full `gpt-<version>-<family>` slug remains a
pin and passes through unchanged.

horch reads the Codex catalog at most once in each process. When
the catalog command fails or has no listed member of a family, horch reports
the fallback and uses the built-in family table.

## Consequences

- A new listed Codex release becomes the fleet default without a roster edit.
- Routing, launch records, telemetry and command lines use the resolved slug.
- Route and spawn output retain the configured family in parentheses.
- The fallback table must be updated when a family needs a newer safe floor.
