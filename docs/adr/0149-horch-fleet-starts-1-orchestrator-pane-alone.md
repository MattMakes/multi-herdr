# horch fleet starts 1 orchestrator pane alone

`horch fleet` (`crates/horch/src/cmd/recipes.rs`) launches the herdr-fleet
workspace with 1 orchestrator pane, alone. It reads the roster, breaks the
work down and spawns exactly the workers it needs with `horch spawn`.
Workers record progress with `horch note` and close their own pane with
`horch done` when truly finished. Earlier versions pre-spawned a fixed 2x2
grid of idle workers. That fixed the shape of the team before anyone knew
what the work was, and burned 4 agent sessions holding a greeting.

## Consequences

- `options.resume` continues an earlier orchestrator session instead of
  starting a new one. Unless `options.no_update`, every agent CLI updates
  first (ARC-32).
- `FleetFlavor` picks which agent, on which model, orchestrates. Only the
  orchestrator pane differs. The roster it spawns workers from is the same
  either way, so a Codex orchestrator still reaches for `opus` when a task
  wants Claude, and a Claude one still reaches for `codex-sol`.
- 2 teammate files back 4 flavors. The flavor picks the file (Claude or
  Codex) and passes its model as the pane's `--model`, which wins over the
  file's own `model:`.
- Opus is the default because it is the cheapest model that orchestrates
  well (cezaar#40 measured Opus 5.5 at $4/$20 per Mtok against Fable 5.1 at
  $10/$50. Fable 5.2 is $8/$40). Fable and Astra stay 1 word away for work
  that needs the top tier.
- `FleetFlavor::model` uses Claude aliases (`opus`, `fable`), which track
  the latest release, but literal Codex slugs (`gpt-6-astra`,
  `gpt-6.1-sol`), because the codex CLI has no alias mechanism.
- `recipes::fleet_with` runs every step that can fail without herdr
  (teammate, session id, skills, ledger record) before it creates the
  workspace, so such a failure leaves nothing to close. From workspace
  creation on, a failure closes the workspace and ends the record, so a
  failed start leaves no orphan workspace or live record (#217).
- herdr closes a workspace together with its last pane. A refused
  `pane-launch` exits at once and the workspace vanishes, so `fleet_with`
  checks that the orchestrator pane still exists before it reports a live
  workspace.
