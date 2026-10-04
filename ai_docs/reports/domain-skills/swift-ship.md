# S4 swift-ship: App Store shipping skills

Unit `swift-ship`, branch `ds/swift-ship`, worker opus-44.
Plan: `ai_docs/plans/domain-skills/s4-swift-ship.md`. Specification:
`ai_docs/reports/swift-fleet-skills-2026-10.md` §4.4, §5, §7.3.

## Outcome

- 8 skills are under `skills/`. `horch skills` (the built binary) lists all 8.
- The gate fails on 3 tests in `crates/horch-core/tests/skills_catalog.rs`. All 3 are V0 items:
  - `skills_bundled_size_budget`: `appstore-review/SKILL.md: 17477 bytes` (V0 exempts `vendored: true`).
  - `skl_01_bundled_catalog_versions_and_digests`: `Skills ... is not a pinned upstream` (V0 checks pins by shape).
- All other test targets pass (60 `test result: ok` lines). `cargo fmt --check` and both builds pass. The gate stops after the test step, so I ran its last 4 steps by hand: `horch teammates --check`, `check-req-coverage.sh`, `check-deps.sh` and `verify-telemetry-e2e.sh` all pass.
- I checked the V0 rules by hand. Every new file is `.md`. There are no dotfiles. Every new source has a GitHub URL, a 40-hex revision, a 64-hex sha256. Only `appstore-review` is over budget, and it is `vendored: true`.

## Safety model

No skill can lead a worker to change App Store Connect. Each live write becomes a dry-run sent to the orchestrator with `horch tell orchestrator`: the exact command, the resolved IDs, and the read-only or `--dry-run` output. A human runs it.

One decision: in `asc-xcode-build`, a build upload (`asc builds upload`, `asc xcode export --wait`) runs only when the assignment names that upload. The reason: the §6 persona `app-release-preparer` is "archive, internal TestFlight upload ... Never submits". The orchestrator gives that authority in the assignment, so the upload stays orchestrator-gated. `asc publish` never runs.

## Per skill

Bytes are SKILL.md / whole directory.

### asc-cli-usage: Adapt, light (3762 / 4832 bytes, `vendored: false`)

- `SKILL.md:3`: "in this repo" removed from the description.
- `SKILL.md:35`: the web-session availability fallback becomes "report it to the orchestrator; a human configures Pricing and Availability". The `asc web apps availability create` example line is removed.
- `SKILL.md:43`: fleet rule added. A command that changes App Store Connect state is not run, with or without `--confirm`. It notes that `asc xcode version edit` changes only local files.
- Removed after `SKILL.md:54`: the web-auth material (upstream `:55-61` `asc web auth capabilities` and `asc web api-keys create`, and `:63-70` "Reuse authentication before requesting another code") and the Apple Ads section (upstream `:72-80`).

### asc-id-resolver: verbatim (3255 / 4325 bytes, `vendored: true`)

- Every command is a read (`list`, `info`). The report's edits are for `cli-usage` only, so this copy needs no edit. The mutation grep finds 0 hits.

### asc-crash-triage: verbatim (3373 / 4443 bytes, `vendored: true`)

- Every command is a read. `asc performance download --output ./metrics.json` writes a local file only. The mutation grep finds 0 hits.

### asc-xcode-build: Adapt, light (9103 / 10173 bytes, `vendored: false`)

- `SKILL.md:16-18`: new section "Fleet rule: App Store Connect writes". Local steps run. An upload runs only when the assignment names it. `asc publish` is never run.
- `SKILL.md:129-130`: the link to the unbundled `asc-ad-hoc-distribution` skill becomes "report the need to the orchestrator".
- `SKILL.md:144`: a fleet-rule line at the head of "3. Upload or publish". It marks the `asc publish` examples as dry-run only.
- `SKILL.md:216`: the link to the unbundled `asc-signing-setup` skill becomes "report the bundle ID and the export error to the orchestrator".

### asc-submission-health: Adapt, diagnosis half (7828 / 14925 bytes, `vendored: false`)

- `SKILL.md:3`: the description drops "operate review health ... cancellation, and retry decisions" and the `asc-release-flow` pointer. It says read-only and dry-run.
- `SKILL.md:10-12`: new section "Fleet rule: diagnosis only".
- Removed: "Ownership boundary" (upstream `:10-19`, about handing work to `asc-release-flow` and continuing authorized repairs).
- "Answer order" item 4: proposing repairs is the only mode. Executing repairs is removed.
- "Route repairs" (`SKILL.md:65-80`): `digital-goods.md` and `app-privacy.md` are not copied, because both are mainly repair procedures. Their read-only inspection commands and their web-session gaps are summarized in a few lines. Game Center goes to the orchestrator. The worker has no web session, so it names the gap and the manual fallback.
- "Cancel or retry: dry-run only" (`SKILL.md:122-132`): the upstream cancel commands and the retry sequence become a dry-run for the orchestrator.
- The routing table (`SKILL.md:136-144`) and the guardrails (`SKILL.md:149`): no `asc-release-flow`, and no cancel without evidence.
- `references/readiness-repairs.md:5`: fleet rule. Only the inspection commands run, and every write goes into the dry-run.
- `references/readiness-repairs.md:100`: the push is sent as a dry-run. "the user has chosen" becomes "the orchestrator has chosen".
- `references/readiness-repairs.md:130`: the `asc-screenshot-resize` and `asc-shots-pipeline` routes become a report to the orchestrator.
- `references/readiness-repairs.md:150`: the `asc web auth login` and `asc web apps availability create` fallback becomes "a human must configure Pricing and Availability".
- `references/readiness-repairs.md:162`: "Ask the user which territories" becomes "Ask the orchestrator".

### asc-metadata-sync: Adapt, dry-run only (9639 / 10709 bytes, `vendored: false`)

- `SKILL.md:3`: the description says dry-run only.
- `SKILL.md:10-14`: new section "Fleet rule: dry-run only". It states plainly that `asc metadata push` without `--confirm` still writes live metadata, so the worker never runs push. It lists every live-write command and every safe command.
- `SKILL.md:38`, `:56`, `:64`, `:70`, `:77`, `:90`, `:97`, `:106`, `:124`, `:145`: each write step says which lines to run (pull, validate, `--dry-run`, plan, approve, status, diff, import, download, export) and which line goes to the orchestrator.
- `SKILL.md:173-176` "Agent behavior": "never run a remote write". Each "the user" becomes "the task" or "the orchestrator".
- The "checked" item: `asc metadata push` writes without `--confirm`. The evidence is the upstream `skills/asc-metadata-sync/SKILL.md:58-61` at `9a093fa`, which reads "Apply after the plan looks correct" and shows `asc metadata push ... --dir "./metadata"` with no `--confirm`. The asc README (read 2026-10-03) does not document `push`. `asc` is not installed on this host, so I could not run `asc metadata push --help`. The fix does not depend on the answer: the worker never runs push without `--dry-run`.

### appstore-review: Adapt, light (17477 / 47821 bytes, `vendored: true`)

- `SKILL.md:4`: the folded (`>`) description is one quoted line.
- `trigger: manual` and `agents:` stay. horch's parser (`crates/horch-core/src/skills/catalog.rs` `Metadata`, not `deny_unknown_fields`) accepts them, and `horch skills` lists the skill. The plan says to drop them only if the parser rejects them.
- `SKILL.md:37-49`: the status set adds UNVERIFIED. A *(runtime)* check needs evidence from this run, else it is UNVERIFIED. Style rules are WARN at most. The audit is read-only and never submits.
- *(runtime)* marks: `SKILL.md:61`, `:64-68`, `:86`, `:110`, `:112`, `:114` (the whole App Store Connect configuration block), `:126`, `:128`, `:213`, `:214`, `:247`.
- WARN at most: `SKILL.md:63` (TODO/FIXME) and `SKILL.md:184` (`print()` and force-unwraps in 1.8).
- Output format, `SKILL.md:297-305`: an Unverified section. The closing line no longer says "Ready for submission". It says that no blocking issue was found in the checks that could be verified.
- The closing section: "If the user mentions" becomes "If the task mentions".
- `references/appstore-review-ref.md` is verbatim. Its GitHub links are community checklists, not skill repositories, so they stay.
- Not copied: `README.md`, and `references/setup-guide.md`, which is install steps with `~/.claude/skills` machine paths.
- `vendored: true`: the references are verbatim and SKILL.md is over 12 KB. This follows the plan's rule.

### app-store-changelog: Adapt, one line (3215 / 5932 bytes, `vendored: false`)

- `SKILL.md:14-21`: the read-only commands from `scripts/collect_release_changes.sh` are inlined (`git describe`, and 2 `git log` commands over `$range`). I ran them in a scratch repository, with a tag (`range=v1..HEAD`) and without one (`range=HEAD`, full history). Both produce the script's output.
- `SKILL.md:37`: "Ask for clarification" becomes a message to the orchestrator with an assumption.
- The `scripts/collect_release_changes.sh` line is removed from Resources. `agents/openai.yaml` is not copied.
- `references/release-notes-guidelines.md:17`: "ask for clarification" becomes "ask the orchestrator". This follows the fleet rule, which also applies to references.

## "checked" and "verify" items

§4.4 has 1 "checked" claim: the metadata push (see asc-metadata-sync). It has no "verify" item. §9 says the `asc` subcommand names for the persona's deny patterns must be checked against `asc --help`. That belongs to P-Swift, and `asc` is not installed here.

## Step 6: mutation-word grep

Command: `grep -rniE "submit|push|delete|expire|revoke|cancel"` over the 8 skill directories (`.md` files). Every hit is a diagnosis, a content check, a read-only command, or a gated write:

- `asc-cli-usage/SKILL.md:16`: `asc search "submit app for review"`. This is local command discovery.
- `asc-cli-usage/SKILL.md:43`: the fleet rule itself.
- `asc-metadata-sync/SKILL.md:12`: the fleet rule.
- `asc-metadata-sync/SKILL.md:61`: push with `--dry-run`. This is safe.
- `asc-metadata-sync/SKILL.md:64` and `:67`: the live push. It is marked "Do not run ... Put it in the dry-run".
- `asc-submission-health/SKILL.md:3`, `:12`, `:149`: the description, the fleet rule and the guardrail.
- `asc-submission-health/SKILL.md:106-107`: `asc submit status`. This is a read.
- `asc-submission-health/SKILL.md:122`, `:124`, `:128`, `:132`, `:144`: the cancel and retry steps. Each is a dry-run proposal.
- `asc-submission-health/SKILL.md:148`: do not use the removed shortcuts.
- `asc-submission-health/references/readiness-repairs.md:5`, `:100`: the fleet rule and the dry-run.
- `asc-submission-health/references/readiness-repairs.md:96`: push with `--dry-run`.
- `asc-submission-health/references/readiness-repairs.md:97`: the live push. It is gated by `:5` and `:100`.
- `asc-xcode-build/SKILL.md:162`: `asc publish appstore ... --submit --confirm`. It is gated by `:16-18` and `:144` (dry-run only).
- `appstore-review/SKILL.md:24`, `:49`, `:100`, `:101`, `:115`, `:147`, `:174`, `:217`, `:265`, `:266`: audit prose and content checks (renewal "canceled" wording, face data "deleted", Push Notification entitlement, submission topology). None is a command.
- `appstore-review/references/appstore-review-ref.md`: 6 lines. They are Apple documentation links and submission-topology prose. None is a command.
- `asc-id-resolver`, `asc-crash-triage`, `app-store-changelog`: 0 hits.

## Provenance and README

- `skills/provenance.json`: 8 entries, sorted by name (the 2 `app*` entries before `art-direction`, the 6 `asc-*` entries after it). Each source file I copied or adapted is listed with its sha256 at the PINS.txt revision. This includes the inlined changelog script, and the 2 submission-health references that are summarized and not copied. Each entry has `vendored` (V0 adds the field; the current parser ignores it).
- `skills/README.md`: a new "Swift and Apple skills" table, sorted, with 8 rows. S1-S3 add their rows to the same table. At rebase, keep both sides.

## Gotchas

- The source files under `.worktrees/_sources/swift/` are mode `0444`. `cp` keeps that mode, so run `chmod u+w` before you edit a copy.
- The crash-triage description says "when the user asks". That is trigger wording, not a human gate, so it stays verbatim.

## Outside my scope (not fixed)

- The `-allowProvisioningUpdates` flag in `asc-xcode-build` can create or update provisioning profiles in the Apple Developer account. That account is not App Store Connect metadata, but it is a live change. P-Swift should decide whether the release persona's key allows it.
- The §6 `disallowed_tools` for `app-release-preparer` does not deny `asc metadata push`. The skill forbids it, but a second line of defence would add `"Bash(asc metadata push *)"` (P-Swift).
