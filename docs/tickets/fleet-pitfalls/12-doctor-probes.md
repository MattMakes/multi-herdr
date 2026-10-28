# 12: Doctor probes teammate services and warns about a proxy (P33, P24)

**Issue:** [#54](https://github.com/MattMakes/multi-herdr/issues/54) · **Spec:** [#46](https://github.com/MattMakes/multi-herdr/issues/46)
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Check:** `cargo build --workspace --bins && cargo test -p horch doctor && cargo test -p horch-core --test arch_scan`

The GitHub issue is the source of truth. This file is its copy at handoff (2026-10-28), so that `horch ledger add-map` can queue it.

## Parent

#46

## What v0.3.0 changed (checked 2026-10-28)

**Still needed as written.** `horch doctor` is unchanged. Nothing checks:

- Docker;
- that MCP server commands are on PATH;
- whether the host can sandbox;
- a Claude base-URL override.

## What to build

`horch doctor` finds broken services before a worker loses a turn to them.

- **Docker.** Teammate requirements gain `docker`. Doctor asks the Docker daemon for its version, with a timeout, when an offered teammate needs it.
- **MCP servers.** For each offered teammate's stdio MCP servers, doctor checks that the server command resolves on PATH. It does not start the server.
- **Sandbox.** When an offered teammate asks for a sandbox, doctor runs the launch's existing host check.
- **Proxy.**
  - The runtime layer reads whether the Claude base URL is overridden, and to which host. This is the only new environment read, and it stays in the runtime layer.
  - Doctor warns when the host is not Anthropic's: plan reads may come back cut short.
  - Nothing reads `ANTHROPIC_API_KEY`.
- All of these are warnings that keep exit code 0. Only an unreachable herdr fails.
- This adds the first end-to-end test of `horch doctor`.

## Acceptance criteria

- [ ] End-to-end: with a teammate that needs Docker and a fake `docker` that hangs, doctor warns within its timeout and exits 0.
- [ ] A teammate whose MCP server command is missing from PATH gets 1 warning naming the teammate and the server.
- [ ] A sandbox teammate on a host that cannot sandbox gets the same message the launch would give.
- [ ] A base-URL override to a non-Anthropic host gets 1 warning; no override, or Anthropic's host, gets none.
- [ ] `teammates --check` accepts `docker` in `requires:`.
- [ ] The arch scan still passes (no environment read outside the runtime layer).

## Blocked by

None (can start immediately)
