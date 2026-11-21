# Fleet ceiling: the teammates lowered to medium on 2026-11-21

The operator's instruction: "Not many models should run higher effort than
Opus 5.5 medium. If they do, I need to personally sign off on them and why,
otherwise, they should default to Opus 5.5".

The fleet ceiling is Opus 5.5 at medium effort
([ADR 0195](adr/0195-nothing-runs-above-opus-5-5-at-medium-without-the-operators-sign-off.md)).
On 2026-11-21 every teammate whose `effort` was above `medium` (39
files) was lowered to `medium`. No model changed. No teammate carries a
`signoff:`: only the operator writes one. This page is what the operator
reads to decide which teammates to sign off.

This page stands in for `ai_docs/reports/tuning-2026-11-21.md`, which the
`tune-fleet` skill names: `ai_docs/` is ignored by git, so the record is
here.

To restore a level, the operator adds 1 line to the teammate file and sets
`effort:` back:

```yaml
signoff: "<yyyy-mm-dd> <who>: <why>"
effort: high
```

`horch teammates --check` fails an effort above `medium` without that line.

## Teammates lowered to medium

The old reason is the `#` comment that stood above `effort:` in the file,
shortened to 1 line. Each file keeps it above its new `effort: medium`.

| teammate | harness | model | old effort | new effort | the old reason |
|---|---|---|---|---|---|
| `addictive-game-design-reviewer` | claude | opus | high | medium | A review that reads a formula wrong, or that proposes a new system where a change of numbers was enough, sends the whole build the wrong way. |
| `apple-accessibility-auditor` | claude | sonnet | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `architect-reviewer` | claude | opus | high | medium | A missed finding costs a review round, which cost more than the review itself in cezaar#40's baseline (reviewers+judges were 71% of worker spend at xhigh); not xhigh: diminishing returns above high. |
| `astra-creative` | codex | gpt-6-astra | high | medium | Open design is where reasoning depth pays. |
| `audio-reviewer` | antigravity | gemini-4-neon (it was gemini-4-argon at high) | high | medium | The probe ran at high, and a missed defect costs a fix round. |
| `blender-qa` | claude | sonnet | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `codex-reviewer` | codex | gpt-5.6-sol | high | medium | Like the claude reviewers, a missed finding costs a review round (cezaar#40); not xhigh or max: diminishing returns above high. |
| `codex-swift-reviewer` | codex | gpt-5.6-sol | high | medium | Like the claude reviewers, a missed finding costs a review round (cezaar#40); not xhigh or max: diminishing returns above high. |
| `comment-purger` | claude | opus | high | medium | Each comment gets a verdict, and a decision filed as a tautology is lost when the comment goes. |
| `design-critic` | claude | opus | high | medium | Like the other reviewers, a missed finding costs a fix round. |
| `design-director` | claude | opus | high | medium | Like the planners, every builder works from this contract, so a weak direction costs a build round per builder. |
| `fable-creative` | claude | fable | high | medium | Open design is where reasoning depth pays. |
| `game-design-incremental-specialist` | claude | opus | high | medium | A wrong model (an additive pool read as a multiplier, a reset that keeps the wrong thing) or a failed design presented as ready gives a confident, wrong verdict that the whole build then follows. |
| `game-design-juicer` | claude | fable | high | medium | Taste and balance calls are where reasoning depth pays; it stayed high on the opus fallback. |
| `godot-code-reviewer` | claude | opus | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `godot-performance-engineer` | claude | opus | high | medium | A profiler reads numbers and must not guess at causes; a wrong hot-spot costs a second capture, the same as ue-qa-engineer. |
| `godot-qa-engineer` | claude | sonnet | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `godot-tech-lead` | claude | opus | high | medium | A planner, like staff-engineer; a wrong scene split or a missed autoload costs every builder after it. |
| `intake-auditor` | claude | opus | high | medium | It judges hostile input. |
| `opus-creative` | claude | opus | high | medium | Open design is where reasoning depth pays. |
| `opus-hardening` | claude | opus | high | medium | Root-cause analysis and hardening need depth, and Opus at high often matches or beats a smaller model at max on fewer tokens. |
| `opus-verify` | claude | opus | max | medium | Fully autonomous, sandboxed, high-stakes verification; the most tokens per task on the roster, so only for a precise spec. |
| `product-lead` | claude | opus | high | medium | Plans and product calls are where a wrong turn is expensive. |
| `qa-engineer` | claude | sonnet | high | medium | A missed finding costs a review round, which cost more than the review itself in cezaar#40's baseline (reviewers+judges were 71% of worker spend at xhigh); not xhigh: diminishing returns above high. |
| `root-cause-auditor` | claude | opus | high | medium | The verdict on each site is a judgement about intent and history; a wrong "patched-around" verdict costs a rewrite of code that was right. |
| `skill-importer` | claude | opus | high | medium | Security judgment and gate decisions. |
| `sonnet-bugfix` | claude | sonnet | high | medium | Targeted bug hunting in one known place; a wrong fix costs a second round, so this seat got the depth to confirm the cause before it edits. |
| `sonnet-sweep` | claude | sonnet | max | medium | Autonomous multi-file runs, where the effort buys consistency across a long run, not insight; only for precisely specified work. |
| `sound-engineer` | claude | opus | high | medium | Each sound is a search over candidates, aimed and checked by ear. |
| `staff-engineer` | claude | opus | high | medium | Plans and product calls are where a wrong turn is expensive. |
| `swift-qa-engineer` | claude | sonnet | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `swift-reviewer` | claude | opus | high | medium | A missed finding costs a review round, the same as architect-reviewer. |
| `ue-code-reviewer` | claude | opus | high | medium | A missed finding costs a review round, the same as architect-reviewer. |
| `ue-qa-engineer` | claude | sonnet | high | medium | A missed finding costs a review round, the same as qa-engineer. |
| `ue-tech-lead` | claude | opus | high | medium | A planner, like staff-engineer; a wrong module split or a missed plugin dependency costs every builder after it. |
| `video-critic` | claude | opus | high | medium | Like the other reviewers, a missed finding costs a fix round. |
| `video-director` | claude | opus | high | medium | Like the other planners, every builder works from this plan. |
| `video-producer` | claude | opus | high | medium | Like the other planners, every wave works from this production plan. |
| `worktree-steward` | claude | sonnet | high | medium | Each row needs a careful read of its diff, but no design work; the operator decides, and the steward only prepares the decision. |

By old level and model:

| old effort | model | teammates | count |
|---|---|---|---|
| high | sonnet | `apple-accessibility-auditor`, `blender-qa`, `godot-qa-engineer`, `qa-engineer`, `sonnet-bugfix`, `swift-qa-engineer`, `ue-qa-engineer`, `worktree-steward` | 8 |
| high | opus | `addictive-game-design-reviewer`, `architect-reviewer`, `comment-purger`, `design-critic`, `design-director`, `game-design-incremental-specialist`, `godot-code-reviewer`, `godot-performance-engineer`, `godot-tech-lead`, `intake-auditor`, `opus-creative`, `opus-hardening`, `product-lead`, `root-cause-auditor`, `skill-importer`, `sound-engineer`, `staff-engineer`, `swift-reviewer`, `ue-code-reviewer`, `ue-tech-lead`, `video-critic`, `video-director`, `video-producer` | 23 |
| high | fable | `fable-creative`, `game-design-juicer` | 2 |
| high | gpt-5.6-sol | `codex-reviewer`, `codex-swift-reviewer` | 2 |
| high | gpt-6-astra | `astra-creative` | 1 |
| high | gemini-4-argon (now gemini-4-neon) | `audio-reviewer` | 1 |
| max | sonnet | `sonnet-sweep` | 1 |
| max | opus | `opus-verify` | 1 |
| | | total | 39 |

4 of these files also changed their text, because the text named the level:
`opus-hardening` ("Opus/high"), `opus-verify` ("Opus/max"), `sonnet-bugfix`
("Sonnet/high") and `sonnet-sweep` ("Sonnet/max") now say "medium" in
`brief_description` and in the first line of the persona. `audio-reviewer`
says "Neon/medium" in `brief_description`: it moved from Gemini 4 Argon to
Gemini 4 Neon on the same day (ADR 0196).

## Seats above the ceiling by model, awaiting sign-off

These worker seats run on a reserved model, which is above Opus 5.5. Their
effort is in the table above and is now `medium`. Their model did not
change, and they keep working: `horch teammates --check` prints a
`warning:` for each and still passes. The operator decides each seat
separately. 1 `signoff:` line covers the seat's model, and its effort if
the operator raises that too.

| teammate | harness | model | effort now | effort before | the seat | the check prints |
|---|---|---|---|---|---|---|
| `astra-creative` | codex | gpt-6-astra | medium | high | Creative council seat on Astra; no fallback | `warning: astra-creative: model 'gpt-6-astra' is above the fleet ceiling (Opus 5.5 at medium) and awaits the operator's sign-off` |
| `fable-creative` | claude | fable | medium | high | Creative council seat on Fable; also combines the 3 designs; no fallback | `warning: fable-creative: model 'fable' is above the fleet ceiling (Opus 5.5 at medium) and awaits the operator's sign-off` |
| `game-design-juicer` | claude | fable | medium | high | Juice proposals for a game or design doc; falls back to `opus` when the Fable window is used up | `warning: game-design-juicer: model 'fable' is above the fleet ceiling (Opus 5.5 at medium) and awaits the operator's sign-off` |

Not in this table, by the rule: the orchestrator teammates. `orchestrator`
(`fable`, medium), `orchestrator-codex` (`gpt-6-astra`, medium) and the
hidden `orchestration-orchestrator` (`fable`, medium) are exempt from the
model rule, because the operator picks the orchestrator's model for each
launch with `horch fleet <flavor>`. `horch spawn --effort` and
`horch spawn --model` are also the operator's own decisions for 1 launch.
They are not roster settings and need no sign-off.

## What to measure to decide a sign-off

No run was measured for this change: it applies the ceiling, and the
numbers come after it. Decide each sign-off from review rounds failed per
role, as the `tune-fleet` skill does (`.claude/skills/tune-fleet/SKILL.md`,
"4. Decide"): a level above medium is worth its cost only where the role's
output at medium keeps failing review, because 1 extra review round usually
costs more than the higher level. Take at least 3 fleet runs after
2026-11-21, and for each run, in the project it ran in, read
`horch cost --since <run start> --text`: the sessions and the mean cost per
teammate, and the cost by family (build, review, research, plan). A lowered
reviewer, planner or QA seat whose work needs more rounds shows more
sessions per task and a larger review share than the same role had before
2026-11-21 (the cezaar#40 baseline was reviewers and judges at 71% of
worker spend). Count the failed rounds themselves per task with
`horch ledger steps T-<n>` and `horch ledger events`: a finish step
(`check`, `code-sweep`, `recheck`) that did not pass, and each fix round
that followed a review. Then, for each row of the first table, compare 3
numbers before and after: failed review rounds per task for that role, the
cost per session of the role, and the cost of the fix rounds its misses
caused. Sign a teammate off when the failed rounds went up and the fix
rounds cost more than the level saves. Leave it at medium when the rounds
did not move. For 1 task that needs more depth, use `horch spawn --effort`
for that launch and change no file.
