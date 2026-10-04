---
name: asc-submission-health
description: Diagnose App Store submission blockers and review health with asc, including readiness validation, repair routing, and status monitoring. Read-only toward App Store Connect; every repair, cancel, retry, or submit becomes a dry-run sent to the orchestrator. Use when validation fails, a version is not in a valid state, or review status is unclear or stuck.
---

# App Store submission health

Use this skill to explain why a release cannot proceed and to report on an existing review submission. This fleet copy keeps the diagnosis half only.

## Fleet rule: diagnosis only

Run only read-only commands: `validate`, `review doctor`, `info`, `view`, `list`, `status`, `history`, and `--dry-run`. Do not run a command that repairs, cancels, retries, or submits, with or without `--confirm`. Send the dry-run to the orchestrator instead with `horch tell orchestrator`: the blocker, the evidence, and the exact proposed command with resolved IDs. A human runs it.

## Answer order

1. State whether the version is ready, blocked, or already under review.
2. Name each blocker and the evidence that proves it.
3. Separate public-API repairs from web-session and manual work.
4. Run the read-only checks needed to establish the diagnosis. Report the evidence and one proposed repair command per blocker, without executing that repair.

## Establish the target

- Resolve `APP_ID`, the version string or `VERSION_ID`, `BUILD_ID`, platform, and any known `SUBMISSION_ID`.
- Configure auth with `asc auth login` or `ASC_*` environment variables.
- Use `ASC_BYPASS_KEYCHAIN=1` only for repository tests and isolated verification, not normal user sessions.
- Prefer IDs once the target is resolved; stop when app, version, or product resolution is ambiguous.

## Diagnose readiness

Run the canonical readiness report first:

```bash
asc validate --app "APP_ID" --version "1.2.3" --platform IOS --output table
```

Use `--version-id "VERSION_ID"` when known. Add `--strict` when warnings must fail automation.

Ask the review-specific doctor for an ordered explanation:

```bash
asc review doctor --app "APP_ID" --version "1.2.3" --platform IOS --output table
```

Collect direct evidence when the report points at the build or version:

```bash
asc builds info --build-id "BUILD_ID" --output table
asc versions view --version-id "VERSION_ID" --include-build --include-submission --output table
```

For digital goods, run only the relevant product validator:

```bash
asc validate iap --app "APP_ID" --output table
asc validate subscriptions --app "APP_ID" --output table
```

Treat the ordered remediation plan from `asc validate` as the repair queue. Report one class of blocker at a time, in that order.

## Route repairs

Public API commands cover build processing, metadata, screenshots, review details, encryption, content rights, age rating, availability, and version-scoped product metadata.

Read [references/readiness-repairs.md](references/readiness-repairs.md) when diagnostics identify one of those common blockers or a first-release availability gap. It gives the read-only inspection for each blocker and the repair command to put in the dry-run.

For an IAP or subscription blocker, inspect the product and its `PREPARE_FOR_SUBMISSION` versions, then report the gap:

```bash
asc iap versions list --iap-id "IAP_ID" --state PREPARE_FOR_SUBMISSION --paginate --output json
asc subscriptions versions list --subscription-id "SUB_ID" --state PREPARE_FOR_SUBMISSION --paginate --output json
asc subscriptions groups versions list --group-id "GROUP_ID" --state PREPARE_FOR_SUBMISSION --paginate --output json
```

The first review of a subscription, and the first IAP of an app, must be attached to the app version's submission. The public API cannot do that first attachment; say that it needs an authenticated Apple web session or a manual selection in App Store Connect.

App Privacy answers cannot be confirmed through the public API. When validation raises an App Privacy advisory, report that a human must confirm the published state on the app's App Privacy page in App Store Connect.

Game Center component or version blockers need the multi-item submission flow. Report them to the orchestrator; do not route them through general readiness or digital-goods repairs.

Web-session commands cover only gaps the public API cannot cover. This worker has no web session: name the gap and the manual App Store Connect fallback.

## Decide whether the version is healthy

A version is healthy when:

- `asc validate` has no blocking issues;
- the attached build is `VALID`;
- metadata, screenshots, app info, review details, content rights, encryption, age rating, pricing, and availability are resolved;
- the relevant `asc validate iap` and/or `asc validate subscriptions` checks have no blocking issues, and the required digital-goods versions are prepared;
- any Game Center version items are prepared;
- App Privacy is confirmed or published.

Do not call a version ready merely because one validator exits successfully. Report any warning that still needs a web-session or manual check.

## Monitor review

Use app-scoped status when the submission ID is unknown:

```bash
asc review status --app "APP_ID" --version "1.2.3" --platform IOS --output table
```

Use exact submission or version IDs when available:

```bash
asc submit status --id "SUBMISSION_ID" --output table
asc submit status --version-id "VERSION_ID" --output table
```

Use the release dashboard for surrounding build and review signals:

```bash
asc status --app "APP_ID" --include builds,appstore,submission,review --output table
```

Use history to distinguish a current stall from earlier rejected or completed submissions:

```bash
asc review history --app "APP_ID" --version "1.2.3" --paginate --output table
```

## Cancel or retry: dry-run only

Do not cancel a submission solely because review is taking longer than expected. When the evidence says the active submission must be withdrawn, send this dry-run to the orchestrator: the `asc submit status` output and the command a human would run, `asc submit cancel --id "SUBMISSION_ID" --confirm` (or `--version-id "VERSION_ID" --app "APP_ID"`).

There is no dedicated retry command. Propose this sequence in the dry-run:

1. Cancel only if the active submission must be withdrawn.
2. Repair the proven blockers.
3. Re-run `asc validate` and the relevant product validators.
4. Confirm no active submission already owns the version or review items.
5. Submit the healthy version, reusing an inspected `READY_FOR_REVIEW` draft and any preserved `SUBMISSION_ID`.

## Common failure routing

| Symptom | First evidence | Repair route (dry-run) |
| --- | --- | --- |
| Version is not in a valid state | `asc validate`, `asc review doctor` | ordered readiness repairs |
| Export compliance must be approved | build info and encryption declaration | readiness repairs |
| Multiple app infos found | `asc apps info list --app "APP_ID"` | resolve exact app-info ID |
| IAP or subscription is not ready | product validator | digital-goods gap, above |
| Game Center component or version is not ready | `asc validate` diagnostic | report to the orchestrator |
| App Privacy publish state is unclear | validation advisory | manual check by a human |
| Review appears stuck | review status plus history | monitor; propose a cancel only with evidence |

## Guardrails

- Do not use removed `submit-preflight` or `submit-create` shortcuts.
- Do not repair, cancel, retry, or submit from this skill; send the dry-run to the orchestrator.
- Do not treat web-session automation as public App Store Connect API coverage.
- Use `--output table` for human diagnosis and JSON for automation.
- For macOS, use `--platform MAC_OS` while keeping the same health lifecycle.
