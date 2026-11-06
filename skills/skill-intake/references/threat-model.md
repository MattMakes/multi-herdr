# Threat model

A skill is instructions plus optional files. An agent with shell, file and
network tools reads it and follows it. The attacker is the skill's author, or
whoever compromised the source or one of its dependencies.

## Assets

- The operator's credentials: shell env, `~/.ssh`, `~/.aws`, the keychain,
  `gh` auth, the Claude login.
- The operator's code and repos.
- The agent's configuration: `~/.claude`, settings, hooks, other skills,
  teammates, `CLAUDE.md`, memory.
- The operator's trust in what the agent reports.

## Attack channels

| # | Channel | Scanner family | Main defence |
|---|---|---|---|
| 1 | Text aimed at the agent (prompt injection), hidden or visible | INJ | the writer never sees source text; the spec carries behavior only |
| 2 | Scripts the skill tells the agent to run | SCR | nothing runs; legitimate behavior is rewritten, the rest removed |
| 3 | Dependencies pulled at install or run time | SUP | each one is replaced with own standard-library code, or removed |
| 4 | Opaque artifacts: binaries, bytecode, wasm, minified or bundled code, vendored packages | OPQ | never executed or extracted; replaced from an inferred purpose, or removed |
| 5 | The intake run itself: hostile files aimed at the extractor, the scanner or the role agents | ACQ, AUD-TAMPER | quarantine, caps, side roots, sandboxes, domain snapshot |

`intake scan` applies the deterministic rules. The rule ids, matches and
severities are in the `intake` scanner. Severity order: `critical` > `high`
> `medium` > `low` > `info`.

## What never runs

- No file under `quarantine/` is executed, imported, loaded or extracted.
  Only `intake` reads it, as bytes or text. The static readers `nm`, `otool`
  and `objdump` get an opaque file only as a file to read, inside a sandbox.
- `intake acquire` runs 1 network operation, a shallow `git clone` with hooks,
  submodules, LFS smudge and symlinks off. It deletes `.git/` after the clone.
- A rewritten script runs only through `intake cr verify`: offline, cwd
  `K/out`, no reads of `C` or `D`, no writes outside `K/out` and a temp dir.

## Trust walls

| Side | Root | Roles | May read | May write |
|---|---|---|---|---|
| contaminated | `C` | controller (no quarantine reads), auditor, analyst | `C` | auditor: `C/sweep/audit.jsonl`, `C/sweep/opaque/*.md`; analyst: `D/drafts` |
| source-blind | `D` | sanitizer | `D` | `D/drafts`, `D/quarantine-cr` |
| clean | `K` | writer, reviewer | `K` | `K/out`, `K/report.json` or `K/review.json`, writer also `K/persona.md` |

Flow is one-way: `C` to `D` (analyst), `D` to `K` only through
`intake cr handoff`, and `K` to `C` only as files the controller reads
through `intake`.

Each role also writes its status file, `<side root>/status/<role>-<n>.json`.
The domain snapshot hashes every file of the 3 roots before and after each
role. A change outside the role's write set is a domain incident, and a
domain incident fails GATE 2.

## Who may change a finding

- `intake scan` and `intake triage` set findings and planned dispositions.
- The auditor can add a finding (`AUD-NN`) or raise a scanner hit. It can
  never lower, close or delete one. `intake triage` drops such a line and
  records `AUD-TAMPER` (high). A line that fails the schema becomes
  `AUD-INVALID` (medium). `intake report` writes 1 incident file for each.
- Only `intake verify` closes a scanner finding, by a mechanical re-sweep of
  the output.
- `K/persona.md` loads into every session of the new teammate. GATE 2 sweeps
  it (SCR on every line), overlap-checks it and needs a reviewer `persona`
  pass; install uses it only when its sha256 matches `C/verify.json`.

## Malicious intent

A source that shows intent to harm is not rewritten, because its
benign-looking parts are suspect too. Intent comes from a scanner hit marked
`malicious` (tag-character smuggling, an imperative in hidden text, the
Anthropic key variable read and sent to the network, exfiltration of
credential files, the keychain or the whole env, decode-then-run) or from an
auditor finding marked `malicious` with a reason. Any malicious finding
aborts at GATE 1, whatever its disposition.

## Never acceptable in output

A re-sweep hit on the output is accepted only inside a declared capability
(see [install](install.md#declared-capabilities)). These rules are never
accepted: SCR-03, SCR-05, SCR-07, INJ-01, INJ-02, INJ-04, INJ-06, SUP-02.

INJ-04 is text that tells the agent to hide an action from the user, drop
earlier instructions, or take a new role. "silently" counts only as an order
("silently delete the entries", "run it silently"). A negation earlier in the
same clause ("do not delete items silently", "never fail silently") or a
failure description ("fails silently", "ignored silently") is not INJ-04. A
clause ends at `. ; ! ? : ,`, a newline, or and/but/or/then/so. An INJ-04
phrase in hidden text, split by invisible code points, on a bidi line or in
tag characters is critical and malicious.
