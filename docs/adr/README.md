# Architecture decision records

Each file records 1 decision: what was decided, why, and what follows from
it. Code in this repository carries no comments
([0004](0004-no-comments-in-code.md)), so a decision that the code cannot
state by itself is recorded here.

To add one, take the next free number, name the file
`NNNN-the-decision-in-kebab-case.md`, follow the form of the existing
records (a title that states the decision, 1 or 2 paragraphs, then
`## Consequences`), and add a line to the list below.

## Foundations

- [0001](0001-machine-wide-sqlite-task-ledger.md) One machine-wide SQLite task ledger at ~/.horch
- [0002](0002-stop-workers-only-by-recorded-pid.md) Stop workers only by their recorded PID and start time
- [0003](0003-two-pane-hosts-behind-one-handle.md) Two pane hosts behind one `PaneHost` handle
- [0004](0004-no-comments-in-code.md) No comments in code; a ratchet removes the old ones

## Across subsystems

- [0005](0005-chrome-starts-with-mac-app-code-sign-clone-disabled.md) Chrome starts with MacAppCodeSignClone disabled
- [0006](0006-code-and-comment-sweeps-run-as-ledger-review-tasks.md) Code and comment sweeps run as ledger review tasks
- [0007](0007-codex-gets-fleet-variables-through-shell-environment-policy-set.md) Codex gets fleet variables through shell_environment_policy.set
- [0008](0008-no-child-process-receives-anthropic-api-key.md) No child process horch starts receives ANTHROPIC_API_KEY
- [0009](0009-parse-proc-stat-fields-after-the-last-parenthesis.md) Parse /proc/<pid>/stat fields after the last parenthesis
- [0010](0010-pi-compaction-summaries-come-from-a-horch-extension.md) pi compaction summaries come from a horch extension
- [0011](0011-tests-run-every-new-executable-once-before-timing-it.md) Tests run every new executable once before timing it
- [0012](0012-the-ledger-owns-its-stored-window-type.md) The ledger owns its stored window type
- [0013](0013-unix-socket-paths-stay-under-the-macos-104-byte-limit.md) Unix socket paths stay under the macOS 104-byte limit
- [0014](0014-values-reach-a-herdr-pane-explicitly-not-through-inherited-environment.md) Values reach a herdr pane explicitly, never through the environment

## Harness: Claude Code

- [0015](0015-claude-command-line-is-built-in-a-fixed-flag-order.md) The claude command line is built in a fixed flag order
- [0016](0016-claude-panes-get-one-settings-overlay-that-sheds-ambient-state.md) Claude panes get 1 settings overlay that sheds the operator's ambient state
- [0017](0017-compaction-is-driven-by-per-harness-capabilities.md) Compaction is driven by per-harness capabilities
- [0018](0018-harness-capabilities-record-what-each-cli-accepts.md) Harness capabilities record what each CLI accepts
- [0019](0019-operator-claude-settings-are-read-with-bounded-reads.md) The operator's Claude settings chain is read with bounded reads
- [0020](0020-plugin-skills-load-a-filtered-copy-of-the-plugin.md) plugin_skills loads a filtered copy of the plugin
- [0021](0021-sandboxed-claude-panes-close-every-tool-to-the-same-paths.md) Sandboxed Claude panes close every tool to the same paths
- [0022](0022-sandboxed-claude-panes-force-sandbox-keys-horch-sets.md) Sandboxed Claude panes force the sandbox keys horch sets

## Harness: Codex

- [0023](0023-codex-fleet-launches-override-config-with-c-pairs.md) Codex fleet launches override config with -c pairs
- [0024](0024-codex-panes-get-a-private-codex-home.md) Codex panes get a private CODEX_HOME that holds only their rules
- [0025](0025-codex-write-panes-run-in-the-fleet-write-permissions-profile.md) Codex write panes run in the fleet_write permissions profile
- [0026](0026-horch-reads-codex-config-with-a-line-scanner-and-profile-files.md) horch reads the codex config with a line scanner and profile files
- [0027](0027-the-state-root-is-a-codex-panes-one-extra-writable-root.md) The state root is a codex pane's 1 extra writable root
- [0028](0028-windows-codex-fleet-rules-go-in-the-shared-rules-file.md) On Windows the codex fleet rules go in the shared rules file

## Harness: launch and the other agent CLIs

- [0029](0029-agent-cli-updates-run-under-a-host-lock-and-are-never-killed.md) Agent CLI updates run under a host lock and are never killed
- [0030](0030-agent-cli-updates-wait-out-a-7-day-release-cooldown.md) Agent CLI updates wait out a 7 day release cooldown
- [0031](0031-antigravity-launches-stay-on-the-operator-google-login.md) Antigravity workers stay on the operator's Google login without skills
- [0032](0032-claude-teammates-are-scoped-by-a-skill-bundle-and-a-settings-overlay.md) Claude teammates get a per-launch skill bundle and keep operator settings
- [0033](0033-each-prime-launch-gets-a-private-agent-dir-of-links.md) Each Prime launch gets a private agent dir of links
- [0034](0034-each-prime-launch-runs-its-own-daemon-that-horch-stops.md) Each Prime launch runs its own daemon that horch stops
- [0035](0035-horch-writes-the-missing-harness-trust-entry-before-launch.md) horch writes the missing harness trust entry before launch
- [0036](0036-one-builder-per-agent-derives-every-launch-from-the-teammate-file.md) One builder per agent derives every launch from the teammate file
- [0037](0037-opencode-effort-goes-through-the-config-overlay.md) OpenCode effort goes through the config overlay
- [0038](0038-pi-workers-load-only-the-horch-compaction-extension.md) pi workers load only the horch compaction extension
- [0039](0039-teammate-paths-and-env-values-expand-tilde-against-the-launch-home.md) Teammate paths and env values expand tilde against the launch home
- [0040](0040-worker-session-ids-are-minted-by-horch-or-recovered-after-launch.md) Worker session ids are minted by horch or recovered after launch

## Execution records and the worker lifecycle

- [0041](0041-a-spawn-never-leaves-a-live-looking-record.md) A spawn never leaves a live-looking record
- [0042](0042-a-worker-that-fails-or-exits-early-always-leaves-word.md) A worker that fails or exits early always leaves word
- [0043](0043-horch-done-records-the-work-before-it-closes-its-own-pane.md) horch done records the work before it closes its own pane
- [0044](0044-live-records-are-judged-per-workspace.md) Live records are judged per workspace
- [0045](0045-panes-are-closed-by-their-public-herdr-id.md) Panes are closed by their public herdr id
- [0046](0046-report-access-panes-are-served-outside-the-sandbox-as-the-brief.md) Report-access panes are served outside the sandbox as the brief
- [0047](0047-session-records-are-keyed-by-record-id-in-the-task-ledger.md) Session records are keyed by record id in the task ledger
- [0048](0048-session-records-stay-byte-compatible-with-older-horch-versions.md) Session records stay byte-compatible with older horch versions
- [0049](0049-workers-reach-horch-by-bare-name-on-path.md) Workers reach horch by bare name on PATH

## Messaging, roster and process identity

- [0050](0050-a-process-is-named-by-its-pid-and-start-time.md) A process is named by its pid and start time, checked where it is stopped
- [0051](0051-a-worker-cannot-forge-the-role-its-messages-carry.md) A worker cannot forge the role its messages carry
- [0052](0052-claude-teammates-start-clean-of-the-operators-settings.md) A Claude teammate can start clean with `--setting-sources ""`
- [0053](0053-codex-workers-run-each-horch-ledger-command-alone.md) Codex workers run each horch ledger command alone
- [0054](0054-every-teammate-states-its-own-effort.md) Every teammate states its own effort
- [0055](0055-fleet-panes-cannot-start-subagents.md) Fleet panes cannot start subagents of their own
- [0056](0056-mail-is-enqueued-durably-in-order-and-at-most-once.md) Mail files are enqueued in order and at most once per key, without a lock
- [0057](0057-messaging-state-lives-in-private-per-workspace-dirs.md) Messaging state lives in private per-workspace dirs under the state root
- [0058](0058-relay-channel-socket-io-is-bounded-by-one-deadline.md) Relay channel socket I/O is bounded by 1 deadline per request
- [0059](0059-roster-check-refuses-settings-an-agent-cannot-honour.md) Roster check refuses teammate settings an agent cannot honour
- [0060](0060-top-model-tiers-are-reserved-for-the-orchestrator-and-named-seats.md) Top model tiers are reserved for the orchestrator and named seats

## Routing and runtime

- [0061](0061-a-broken-harness-binary-is-judged-by-harness-not-by-pool.md) A broken harness binary is judged by harness, not by pool
- [0062](0062-a-herdr-hidden-from-stat-runs-by-its-absolute-path.md) A herdr hidden from stat runs by its absolute path
- [0063](0063-antigravity-quota-windows-are-scoped-by-model-group.md) Antigravity quota windows are scoped by model group
- [0064](0064-child-processes-get-a-scrubbed-environment.md) Child processes get a scrubbed environment
- [0065](0065-detached-children-start-a-new-session.md) Detached children start a new session
- [0066](0066-the-task-ledger-path-is-independent-of-the-state-root.md) The task ledger path is independent of the state root
- [0067](0067-usage-limits-are-read-only-through-each-harness-client.md) Usage limits are read only through each harness's own client
- [0195](0195-agy-teammates-run-gemini-4-neon.md) The agy teammates run Gemini 4 Neon, and `neon` is a quota scope

## Skills, sweeps, compaction and files

- [0068](0068-briefings-are-roster-files-rendered-by-single-pass-substitution.md) Briefings are roster files rendered by single-pass substitution
- [0069](0069-compaction-runs-as-one-file-claimed-job-per-role.md) Compaction runs as 1 file-claimed job per role
- [0070](0070-cross-process-locks-are-directories-published-with-their-owner.md) Cross-process locks are directories published with their owner
- [0071](0071-digests-hash-canonical-json-with-sorted-keys.md) Digests hash canonical JSON with sorted keys
- [0072](0072-durable-and-bounded-file-io-goes-through-fsx.md) Durable and bounded file I/O goes through fsx
- [0073](0073-horch-reads-the-time-from-one-pinnable-clock.md) horch reads the time from 1 pinnable clock
- [0074](0074-identifiers-are-typed-values-with-fixed-formats.md) Identifiers are typed values with fixed formats
- [0075](0075-long-lived-processes-record-the-binary-they-run.md) Long-lived processes record the binary they run
- [0076](0076-project-notes-are-keyed-by-the-common-git-directory.md) Project notes are keyed by the common git directory
- [0077](0077-run-cost-is-read-from-harness-transcripts-by-session-id.md) Run cost is read from harness transcripts by session id
- [0078](0078-teammate-skills-resolve-from-a-validated-digest-pinned-catalog.md) Teammate skills resolve from a validated, digest-pinned catalog
- [0079](0079-the-sweep-removes-state-only-when-its-owner-is-provably-gone.md) The sweep removes state only when its owner is provably gone

## Task ledger

- [0080](0080-a-cancel-or-stop-settles-the-ledger-before-any-signal.md) A cancel or stop settles the ledger before any process is signalled
- [0081](0081-a-dead-orchestrator-respawns-rarely-and-under-guards.md) A dead orchestrator respawns rarely and under guards
- [0082](0082-a-land-fast-forwards-the-run-branch-under-a-repository-lock.md) A task-ledger land fast-forwards the run branch under a land lock
- [0083](0083-a-running-watchdog-is-replaced-only-when-its-binary-changed.md) A running watchdog is replaced only when its binary changed
- [0084](0084-a-wake-is-an-outbox-row-first-and-a-send-second.md) A wake is an outbox row first and a send second
- [0085](0085-codex-worker-checks-run-in-the-pane-sandbox.md) A Codex worker's check runs in the sandbox of its pane
- [0086](0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md) The complexity sweep sends each language to a tool that parses it
- [0087](0087-each-run-has-one-pull-request-with-a-section-per-task.md) Each run has 1 pull request with a section per landed task
- [0088](0088-finish-keeps-a-passed-step-while-its-judged-tree-stands.md) The finish pipeline keeps a passed step while its judged tree stands
- [0089](0089-ledger-git-calls-never-force.md) The ledger's git calls never force and never lose work
- [0090](0090-ledger-paths-are-matched-in-canonical-form.md) Ledger paths are stored and matched in canonical form
- [0091](0091-ledger-read-then-writes-run-in-immediate-transactions.md) Every ledger read-then-write runs in a BEGIN IMMEDIATE transaction
- [0092](0092-ledger-schema-migrates-forward-from-a-frozen-version-1.md) The ledger schema migrates forward in place from a frozen version 1
- [0093](0093-ledger-status-words-come-from-the-model-enums.md) Ledger status words come from the model enums, not SQL literals
- [0094](0094-old-json-ledgers-are-imported-without-changing-them.md) Old JSON ledgers are imported without changing them
- [0095](0095-queued-tasks-are-planned-greedily-in-claim-order.md) Queued tasks are planned greedily in claim order
- [0096](0096-the-watchdog-reads-the-agents-on-screen-timer.md) The watchdog reads the agent's on-screen timer to see a long turn
- [0097](0097-worktree-triage-never-loses-work.md) Worktree triage never loses work

## Telemetry and version control

- [0098](0098-current-context-is-read-from-the-transcript-tail.md) Current context is read from the transcript tail
- [0099](0099-git-runs-through-a-typed-client-that-refuses-option-like-refs.md) Git runs through a typed client that refuses option-like refs
- [0100](0100-landing-is-serialized-under-the-repository-registry-lock.md) Worktree-claim landing is serialized under the claim registry lock
- [0101](0101-one-collector-runs-behind-a-directory-lock.md) One collector runs behind a directory lock
- [0102](0102-remote-git-commands-get-stall-limits-and-a-wall-clock-bound.md) Remote git commands get stall limits and a wall-clock bound
- [0103](0103-snapshot-costs-match-the-store-to-the-last-bit.md) Snapshot costs match the store to the last bit
- [0104](0104-telemetry-events-are-synced-before-cursors-are-saved.md) Telemetry events are synced before cursors are saved
- [0105](0105-telemetry-events-carry-ids-stable-across-re-reads.md) Telemetry events carry ids stable across re-reads
- [0106](0106-the-collector-reads-only-inputs-whose-stamp-changed.md) The telemetry collector reads only inputs whose stamp changed
- [0107](0107-token-classes-are-priced-without-double-counting-or-guessing.md) Token classes are priced without double counting or guessing

## Workspace and pane layout

- [0108](0108-a-closing-pane-re-tiles-from-a-detached-child-in-a-new-session.md) A closing pane re-tiles from a detached child in a new session
- [0109](0109-hc-calls-strip-terminal-identity-and-never-start-a-core.md) hc calls strip terminal identity and never start a core
- [0110](0110-herdr-and-hc-calls-run-in-a-contained-process-tree.md) herdr and hc calls run in a contained process tree
- [0111](0111-herdr-focus-moves-only-by-neighbour-steps.md) herdr focus moves only by neighbour steps
- [0112](0112-herdr-pane-move-keeps-the-pane-and-creates-tabs.md) herdr `pane move` keeps the pane and is how horch creates tabs
- [0113](0113-horch-tile-parks-every-worker-in-a-scratch-tab-then-places-it.md) `horch tile` parks every worker in a scratch tab, then places it
- [0114](0114-pane-command-lines-drop-forbidden-env-before-they-run.md) Pane command lines drop `FORBIDDEN_ENV` before they run
- [0115](0115-rearranging-panes-checks-for-a-zoomed-tab-first.md) Rearranging panes checks for a zoomed tab first
- [0116](0116-resize-loops-re-plan-from-herdrs-reply-after-every-resize.md) Resize loops re-plan from herdr's reply after every resize
- [0117](0117-text-reaches-a-running-agent-only-through-herdr-agent-prompt.md) Text reaches a running agent only through `herdr agent prompt`
- [0118](0118-the-herdr-wrapper-targets-herdr-0-6-1-and-tiling-needs-0-8-2.md) The herdr wrapper targets herdr 0.6.1, and only tiling needs 0.8.2
- [0119](0119-tiling-restores-the-view-by-following-the-focused-pane.md) Tiling restores the operator's view by following the focused pane
- [0120](0120-worker-columns-are-equalised-to-absolute-targets.md) Worker columns are equalised to absolute targets

## Core tests

- [0121](0121-an-execution-record-never-loses-a-recorded-end.md) An execution record never loses a recorded end
- [0122](0122-architecture-rules-are-enforced-by-tests.md) Architecture rules are enforced by tests
- [0123](0123-briefing-changes-carry-the-incident-that-caused-them.md) Briefing changes carry the incident that caused them
- [0124](0124-built-in-teammates-launch-with-fixed-safety-defaults.md) Built-in teammates launch with fixed safety defaults
- [0125](0125-context-readers-treat-harness-data-as-untrusted.md) Context readers treat harness data as untrusted
- [0126](0126-integration-tests-reproduce-host-and-outside-program-quirks.md) Integration tests reproduce host and outside-program quirks
- [0127](0127-test-goldens-and-oracles-are-frozen-captures-of-old-output.md) Test goldens and oracles are frozen captures of the old output
- [0128](0128-the-telemetry-collector-reads-a-cold-corpus-in-batches.md) The telemetry collector reads a cold corpus in batches

## End-to-end tests

- [0129](0129-a-worker-reports-its-startup-error-through-a-file.md) A worker reports its startup error through a file
- [0130](0130-e2e-teardown-ends-every-process-the-harness-started.md) E2e teardown ends every process its harness started
- [0131](0131-fake-herdr-runs-each-call-detached-under-a-state-lock.md) fake-herdr runs each call detached under a dead-holder-only state lock
- [0132](0132-horch-done-finishes-inside-the-agents-120-s-command-timeout.md) horch done finishes inside the agent's 120 s command timeout
- [0133](0133-orchestrator-mail-goes-through-the-channel-relay-while-it-is-live.md) Orchestrator mail goes through the channel relay while it is live
- [0134](0134-task-ledger-worktrees-are-matched-by-real-path.md) Task-ledger worktrees are recorded and looked up in the ledger
- [0135](0135-the-e2e-harness-seals-each-test-with-linked-fakes.md) The e2e harness seals each test in its own root with linked fakes

## The horch commands

- [0136](0136-a-claim-names-an-ignored-ticket-by-its-absolute-path.md) A claim names an ignored ticket by its absolute path
- [0137](0137-a-failed-skill-install-never-echoes-its-source-spec.md) A failed skill install never echoes its source spec
- [0138](0138-a-herdr-restored-pane-may-act-only-as-its-registered-worker.md) A herdr-restored pane may act only as its registered worker
- [0139](0139-a-spawn-only-controller-is-identified-by-its-pane-and-process.md) A spawn_only controller is identified by its pane and process
- [0140](0140-bulk-output-exits-quietly-on-a-closed-pipe.md) Bulk output exits quietly on a closed pipe
- [0141](0141-cancel-a-task-before-stopping-its-worker.md) Cancel a task before stopping its worker
- [0142](0142-codex-panes-escape-the-sandbox-only-by-an-exact-execpolicy-prefix.md) Codex panes escape the sandbox only by an exact execpolicy prefix
- [0143](0143-courier-redelivery-is-bounded-and-never-floods-the-orchestrator.md) Courier redelivery is bounded and never floods the orchestrator
- [0144](0144-finish-steps-run-in-a-detached-process-group.md) Finish steps run in a detached process group
- [0145](0145-herdrs-live-answer-under-a-deadline-decides-what-a-pane-is.md) herdr's live answer under a deadline decides what a pane is
- [0146](0146-horch-compact-runs-as-a-detached-job-with-a-start-handshake.md) horch compact runs as a detached job with a start handshake
- [0147](0147-horch-cost-is-informational-and-never-prices-unknowns-as-zero.md) horch cost is informational and never prices unknowns as zero
- [0148](0148-horch-doctor-reports-host-problems-that-would-stall-a-pane.md) horch doctor reports host problems that would stall a pane
- [0149](0149-horch-fleet-starts-1-orchestrator-pane-alone.md) horch fleet starts 1 orchestrator pane alone
- [0150](0150-horch-install-places-a-real-binary-by-rename.md) horch install places a real binary by rename
- [0151](0151-horch-mode-switches-a-codex-sandbox-through-its-tui.md) horch mode switches a Codex sandbox through its TUI
- [0152](0152-horch-spawn-validates-before-it-touches-herdr.md) horch spawn validates before it touches herdr
- [0153](0153-horch-worker-runs-the-agent-cli-as-a-child-process.md) horch worker runs the agent CLI as a child process
- [0154](0154-messages-are-typed-into-panes-and-a-push-is-never-a-receipt.md) Messages are typed into panes and a push is never a receipt
- [0155](0155-npm-registry-documents-are-read-gzipped-under-a-byte-cap.md) npm registry documents are read gzipped under a byte cap
- [0156](0156-teammates-are-files-in-a-roster-directory.md) Teammates are files in a roster directory
- [0157](0157-the-horch-binary-reads-its-environment-only-in-bootstrap.md) The horch binary reads its environment only in bootstrap
- [0158](0158-the-horch-integration-tests-run-the-real-binary-in-isolation.md) The horch integration tests run the real binary in isolation
- [0159](0159-the-task-ledger-watchdog-runs-detached-and-stops-between-passes.md) The task-ledger watchdog runs detached and stops between passes
- [0160](0160-the-telemetry-collector-holds-its-lock-for-its-whole-life.md) The telemetry collector holds its lock for its whole life
- [0161](0161-undelivered-lines-wait-as-durable-mail-for-a-courier.md) Undelivered lines wait as durable mail for a courier

## Installer, docs sync, marketplace and CI

- [0162](0162-git-children-select-their-repository-only-by-the-c-directory.md) Git children select their repository only by the -C directory
- [0163](0163-herdr-docs-sync-lists-pages-from-the-sitemap-and-prunes-only-from-a-complete-run.md) herdr-docs-sync lists pages from the sitemap and prunes only from a complete run
- [0164](0164-herdr-docs-sync-reserialises-starlight-markup-before-conversion.md) herdr-docs-sync re-serialises Starlight markup before converting it
- [0165](0165-herdr-install-is-a-rust-port-that-reads-herdr-update-manifests.md) herdr-install is a Rust port that reads the manifests of herdr update
- [0166](0166-herdr-install-mirrors-the-official-windows-junction-layout.md) herdr-install mirrors the official versioned junction layout on Windows
- [0167](0167-marketplace-store-changes-only-under-a-guard-directory-lock.md) The marketplace store changes only under a guard directory lock
- [0168](0168-marketplace-treats-skill-sources-and-lock-entries-as-untrusted-input.md) The marketplace treats skill sources and lock entries as untrusted input

## Scripts and bundled skills

- [0169](0169-bundled-skill-scripts-write-no-bytecode-and-import-only-their-own-code.md) Bundled skill scripts write no bytecode and import only their own code
- [0170](0170-code-sent-to-the-page-uses-only-function-declarations.md) Code sent to the page uses only function declarations
- [0171](0171-context-and-telemetry-checks-correct-for-each-clis-own-reporting.md) Context and telemetry checks correct for each CLI's own reporting
- [0172](0172-engine-live-checks-read-failures-where-each-engine-reports-them.md) Engine live checks read failures where each engine reports them
- [0173](0173-godot-csharp-checks-run-only-on-the-dotnet-engine.md) Godot C# checks run only on the .NET engine
- [0174](0174-godot-gameplay-checks-wait-1-frame-before-touching-nodes.md) Godot gameplay checks wait 1 frame before touching nodes
- [0175](0175-hark-modules-import-only-their-own-layer.md) hark modules import only their own layer
- [0176](0176-hark-results-stay-reproducible-across-versions.md) hark results stay reproducible across versions
- [0177](0177-harness-live-checks-send-only-dummy-prompts-on-the-operators-logins.md) Harness live checks send only dummy prompts on the operator's logins
- [0178](0178-live-checks-touch-only-their-own-prime-daemon.md) Live checks touch only their own Prime daemon
- [0179](0179-materials-capture-reaches-the-network-only-when-the-operator-allows-it.md) Materials capture reaches the network only when the operator allows it
- [0180](0180-progression-scripts-measure-the-game-by-its-own-code-and-players.md) Progression scripts measure the game by its own code and players
- [0181](0181-releases-build-from-the-tip-of-main-and-lint-the-release-profile.md) Releases build from the tip of main and lint the release profile
- [0182](0182-remove-zsh-fleet-function-edits-the-startup-file-only-when-safe.md) remove-zsh-fleet-function edits the startup file only when safe
- [0183](0183-repository-scripts-pin-the-environment-that-git-and-horch-read.md) Repository scripts pin the environment that git and horch read
- [0184](0184-scripts-run-unchanged-on-macos-and-gnu-hosts.md) Scripts run unchanged on macOS and GNU hosts
- [0185](0185-skill-intake-data-flows-one-way-from-c-to-d-to-k.md) skill-intake data flows one way from C to D to K
- [0186](0186-skill-intake-passes-git-and-its-tests-only-the-environment-they-need.md) skill-intake passes git and its tests only the environment they need
- [0187](0187-skill-intake-treats-every-source-file-as-untrusted.md) skill-intake treats every source file as untrusted
- [0188](0188-the-hark-fit-bank-ships-only-after-the-operator-allows-the-cc0-downloads.md) The hark fit bank ships only after the operator allows the CC0 downloads
- [0189](0189-the-lamp-road-core-is-pure-and-step-size-invariant.md) The Lamp Road core is pure and step-size invariant
- [0190](0190-the-linux-live-check-tests-a-committed-snapshot-as-a-normal-user.md) The Linux live check tests a committed snapshot as a normal user
- [0191](0191-the-overlap-check-reads-prose-with-a-6-word-window.md) The overlap check reads prose with a 6-word window
- [0192](0192-the-tuning-bench-never-changes-the-running-game.md) The tuning bench never changes the running game
- [0193](0193-video-frames-and-audio-are-computed-from-exact-time.md) Video frames and audio are computed from exact time
- [0194](0194-video-scripts-drive-headless-chrome-with-bounded-starts-and-stops.md) Video scripts drive headless Chrome with bounded starts and stops
