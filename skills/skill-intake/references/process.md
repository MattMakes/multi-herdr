# Process

## Stages

| # | Stage | Who | Writes | Gate |
|---|---|---|---|---|
| 1 | acquire | `intake acquire` | `C/quarantine/src/` (read-only), `C/provenance.json` | refuses unsafe archives and trees over a cap |
| 2 | discover | `intake discover` | `C/skills.json` | 0 skills: stop |
| 3a | scan | `intake scan` | `C/sweep/scan.jsonl`, `C/sweep/opaque/<id>.json` | - |
| 3b | audit | auditor | `C/sweep/audit.jsonl`, `C/sweep/opaque/<id>.md` | - |
| 4 | triage | `intake triage` | `C/sweep/findings.json`, `C/mitigation.json`, `C/sweep/report.md` | GATE 1 |
| 5 | ask | controller | the mapping answer | multi-herdr only |
| 6 | index | `intake cr init`, `intake cr index` | `C/contaminated/preflight.json`, `source-index.json`, `fingerprints.json` | - |
| 7 | specs | analyst, 1 per unit | `D/drafts/spec-*.json` | - |
| 8 | scan, sanitize | `intake cr scan`, sanitizer | `D/drafts/`, `D/quarantine-cr/` | at most 2 sanitizer passes per draft |
| 9 | handoff | `intake cr handoff` | `K/handoff/` | hashes recorded |
| 10 | write | writer, 1 per skill | `K/out/<skill>/`, `K/report.json`, `K/persona.md` | - |
| 11 | review | reviewer, 1 fresh per skill | `K/out/<skill>/`, `K/review.json` | - |
| 12 | verify | `intake cr verify`, `intake verify` | `K/verify-log/log.json`, `C/verify.json` | GATE 2 |
| 13 | install | `intake install` | the target | collision rules |
| 14 | report | `intake report` | `C/report.md` | - |

`intake run <source>` runs stages 1 to 4 in 1 call. Each stage's JSON goes to
stderr. stdout gets `{"task_dir", "stage", "exit"}`, and the exit code is the
code of the first stage that stops.

Units: `intake cr index` makes 1 unit per skill and 1 more unit per script
over 300 lines. 1 analyst works on 1 unit.

## Exit codes

Read each code together with its command. The same number can mean
different things in different commands (for example 14).

| Command | 0 | Other codes |
|---|---|---|
| `acquire` | ok, prints `{"task_dir"}` | 2 bad input; 3 refused (ACQ-01 archive or ACQ-04 cap; nothing is kept) |
| `discover` | ok | 6 no skill found: stop, nothing to do |
| `scan` | ok (findings are data) | - |
| `triage` | GATE 1 pass | 4 GATE 1 abort; 14 pending-audit: no `sweep/audit.jsonl` yet, so run the auditor, then triage again |
| `run` | front half done, GATE 1 pass | the code of the stage that stopped: 2, 3, 6, 4 or 14 |
| `candidates` | ok | - |
| `cr init`, `cr index`, `cr handoff` | ok | 1 a draft rejected (`handoff`); 2 bad input, or GATE 1 is not pass |
| `cr scan` | every file clean | 1 a leak; 2 a path in `C` or outside `D` and `K` |
| `cr verify` | the command passed | its own exit code; 2 GATE 1 is not pass, or an argv into `C` or `D`; 13 no usable sandbox, nothing ran (`unverified`) |
| `cr status`, `cr validate` | ok | 1 a handoff integrity failure or an invalid artifact |
| `cr role set` | ok | 10 refused in fleet mode |
| `verify` | GATE 2 pass | 2 GATE 1 is not pass, or bad input; 5 GATE 2 abort |
| `install` | installed | 2 bad input, or GATE 2 is not pass; 7 a name collision no rule resolves; 15 the repo is on its default branch, nothing written |
| `report` | ok | - |
| `mode` | prints `fleet` or `plain` | - |
| `brief` | prints `{"brief", "status_file"}` | 2 bad input; 9 a clean brief failed the leakage scan |
| `spawn` | role worker started | 2 bad input; 8 the role stage aborted (a brief was lost twice); 9 brief leak; 10 not in fleet mode; 11 `horch spawn` failed; 14 another role of the task is active |
| `roles` | lists role workers | 2 bad input; with `--lost`: 8 the brief is lost a second time, abort; 10 not in fleet mode; 11 `horch spawn` failed |
| `role-result` | the result | 12 a bad status file, a missing status file (the role stays `running`), or a domain incident |
| `subagent-prompt` | prints the prompt | 2 bad input or no role file; 8 aborted; 10 in fleet mode; 14 another role is active |
| `hook-config` | prints the snippet | 10 in fleet mode |
| `guard` | allows the tool call | 2 blocks it |

Every command exits 2 on a bad task path or a bad argument.

## Auditor output

`C/sweep/audit.jsonl` has 1 JSON object per line. `kind` selects the shape.
Every other key is rejected.

| `kind` | Keys | Meaning |
|---|---|---|
| `purpose` | `skill`, `purpose` (at most 200 characters) | a neutral one-line purpose per skill, used in the mapping question |
| `finding` | `rule` (`AUD-NN`), `severity`, `skill` (or null), `note`, `malicious`; optional `file`, `line`, `reason`, `disposition` (`rewrite`, `replace`, `remove` or `none`), `whole_skill` | a new finding the rules miss |
| `raise` | `target` (a scanner hit id, for example `SCR-06#2`), `severity`, `reason`; optional `malicious` | raise a scanner hit |

A line that lowers, closes or deletes a scanner hit becomes `AUD-TAMPER`
(high). A line that fails the schema becomes `AUD-INVALID` (medium).
A `finding` with `disposition: none` and severity high or critical has no
possible disposition and aborts GATE 1. A `finding` with `whole_skill: true`
and `disposition: remove` removes that skill. When every skill is removed,
GATE 1 aborts.

The opaque inference file `C/sweep/opaque/<scan id>.md` (the scan id is the
hit id, for example `OPQ-01#1`) holds: inferred purpose, evidence, confidence
(`high`, `medium` or `low`), risky capabilities seen, and 1 line
`Disposition: replace` or `Disposition: remove`. There is no `keep`. Lines
that disagree give `remove`. An OPQ-01 to OPQ-05 hit with no disposition
line aborts GATE 1. OPQ-06 data files default to `remove`.

## Planned dispositions (triage)

| Finding | Disposition |
|---|---|
| INJ | `rewrite`: the spec states only the legitimate behavior |
| SCR-03, -04, -05, -07, -09, -10, -12 | `remove` |
| other SCR | `rewrite`, with any access declared as a capability |
| SUP-01, -02, -03, -07 | `replace` with own standard-library code |
| other SUP | `remove` |
| OPQ | the auditor's disposition |
| AUD | the auditor's disposition (`none` is unresolved when high or critical) |
| ACQ | `removed` (already skipped) |

## Roles

| Role | Count | Side | `--unit` / `--skill` |
|---|---|---|---|
| auditor | 1 per task | contaminated | - |
| analyst | 1 per unit | contaminated | `--unit U` |
| sanitizer | 1 fresh per pass | source-blind | - |
| writer | 1 per skill | clean | `--skill S` |
| reviewer | 1 fresh per skill | clean | `--skill S` |

Roles run 1 at a time per task, in this order. `C/roles.jsonl` records each
start and end. `intake roles --task T` lists every role worker with its state
(`running`, `done`, `error`, `lost`). `--pending` lists the running ones.

A role never resumes. When a role stops, reports `error`, or reaches its
context limit, start a fresh role of the same kind with the same `--unit` or
`--skill`. A role in fleet mode cannot compact. The fresh role finds the
outputs so far in its write paths.

The role teammate is `intake-<role>`. Each has no network and reads and
writes only its own side root. In fleet mode no guard hook runs. In plain
mode the domain snapshot is the floor, and the guard hook is optional.

## Abort handling

| Abort | Command | Do |
|---|---|---|
| refused source | `acquire` exit 3 | `intake report`; send the reason |
| no skills | `discover` exit 6 | `intake report`; nothing to do |
| GATE 1 | `triage` exit 4 | `intake report`; install nothing, commit nothing |
| a brief lost twice | `spawn` or `roles --lost` exit 8 | `intake report`; install nothing |
| GATE 2 | `verify` exit 5 after the feedback loop | `intake report`; install nothing |

Send the report path and the summary in every case.

## Report

`intake report --task T [--gate-result <result>]` writes `C/report.md`. It
works after any outcome. It holds:

- the outcome: `installed`, `installed with flags`, or `aborted at GATE n`;
- the source, ref, commit or archive hash, the licence and its flag
  (`copyleft`, `no-licence`);
- the mode and the rule that decided it, the install targets and every rename;
- multi-herdr: the branch, the commits, the repo gate result, the mapping
  answer, and "run `/push-pr` to publish";
- the findings: counts by severity, then 1 row per finding (id, severity,
  skill, disposition, verify status), evidence only as escaped excerpts;
- each opaque artifact with its inferred purpose, confidence and disposition;
- each removed feature, 1 line each;
- the declared capabilities accepted;
- leakage results, incidents, whether the guard hook and an OS sandbox were
  active, and the scripts left `unverified`.

`intake report` detects the guard hook only from the settings files.
It records the repo gate result only when you pass `--gate-result`.
