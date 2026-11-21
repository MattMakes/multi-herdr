# Nothing runs above Opus 5.5 at medium without the operator's sign-off

The fleet ceiling is Opus 5.5 at medium effort. The operator's instruction
of 2026-11-21: "Not many models should run higher effort than Opus 5.5
medium. If they do, I need to personally sign off on them and why,
otherwise, they should default to Opus 5.5". A teammate file goes above the
ceiling only with a `signoff:` line that the operator wrote. Before this
decision, effort followed the role, and reviewers, planners, QA and the
creative seats ran at high or max by default
([0054](0054-every-teammate-states-its-own-effort.md), cezaar#40).

The sign-off is 1 optional frontmatter key on the teammate:
`signoff: "<yyyy-mm-dd> <who>: <why>"`. It is 1 line: the date, who signed,
and the reason. The rules live in
`crates/horch-core/src/roster/ceiling.rs` and run from `ceiling_rules` in
`crates/horch-core/src/roster/validation.rs`.

## Consequences

- **Only the operator writes a sign-off.** An agent, a skill or a script
  never adds, edits or copies one. The `tune-fleet` skill proposes a level
  above medium to the operator and stops. horch cannot tell who typed a
  line, so the rule is held by review: a `signoff:` line in a diff is a
  line the operator must have written.
- **An effort above medium without a sign-off fails the check.**
  `horch teammates --check` fails a teammate whose `effort` is `high`,
  `xhigh` or `max` and that has no `signoff`. The rule is the same on every
  harness. Effort levels are not comparable across vendors, so the ceiling
  is the level name, not a measured cost.
- **A malformed sign-off fails.** The check fails a sign-off that is empty,
  has more than 1 line, does not start with a real `yyyy-mm-dd` date, names
  nobody before `: `, or gives no reason after `: `.
- **A stale sign-off fails.** A sign-off on a teammate that is not above
  the ceiling says "remove it". A sign-off that outlives the level it
  allowed would read as permission for the next raise.
- **A reserved model on a worker seat warns; it does not fail.** `fable`
  and `gpt-6-astra` are above Opus 5.5
  ([0060](0060-top-model-tiers-are-reserved-for-the-orchestrator-and-named-seats.md)).
  A worker seat on one of them without a sign-off gets a `warning:` line
  from the check (`ceiling_warnings`) and keeps working. The operator
  decides these seats (`TOP_TIER_SEATS`) separately, and a failing check
  would stop the creative council until then. 1 sign-off covers the seat's
  model and its effort.
- **The orchestrators are exempt from the model rule.** A teammate on the
  `fleet-orchestrator` base, and the hidden `orchestration-orchestrator`,
  may name a reserved model without a sign-off: the operator picks the
  orchestrator's model for each launch with `horch fleet <flavor>`. They are
  not exempt from the effort rule.
- **The operator's one-launch flags are not roster settings.**
  `horch spawn --effort`, `horch spawn --model` and the `horch fleet
  <flavor>` choice are the operator's own decisions for 1 launch. The
  roster check does not judge them, and they need no sign-off. The ledger
  records the level each session ran at.
- **A fallback launch is not judged a second time.** `fallback_problems`
  checks a teammate merged with its fallback for what the fallback's
  harness can launch. The ceiling is a rule about the teammate file, so it
  runs once, on the file.
- **An operator overlay is a roster file.** A file in
  `~/.config/horch/teammates/` or `HORCH_TEAMMATES_DIR` with an effort above
  medium fails the check and `horch doctor` in the same way.
- **On 2026-11-21 every teammate above medium was lowered to medium.** The
  model of each stayed. Each file keeps, in a `#` comment above `effort:`,
  the level it had and the reason, so the operator can decide a sign-off.
  The full table is in `docs/fleet-ceiling-2026-11-21.md`.
- `horch teammates --matrix` shows a sign-off in its notes column, and
  `--json` carries it as `signoff`.
