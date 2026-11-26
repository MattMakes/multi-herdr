# Theme spec record

A theme record proves that a person accepted the theme values, and names the check that can fail for each claim. It sits beside `DESIGN.md` (for example `design/theme.spec.md`). It does not replace `DESIGN.md`: `DESIGN.md` says how to build a screen, the record says why the values are right and how to see when they stop being right. The record never copies token values. It points at the source file.

## Front matter

| Field | Value | Proves |
|---|---|---|
| `id` | unique theme id, `kind: theme` | The record is unique and findable by a tool |
| `status` | `current`, `superseded` or `archived` | Which record governs now |
| `approved_by`, `approved_on` | a named person and a date | A person accepted the values on that date |
| `checked_by` | list of test files | Each claim has an automated check that can fail |
| `recheck_when` | list from `tokens`, `palette`, `component mappings`, `contrast`, `artifacts` | Which change forces a new review |
| `source` | path of the theme source (token file) | Where the exact values live |
| `references` | token, inheritance and build records the theme obeys | Which shared rules apply |

## Body sections, in this order

1. Intent and audience.
2. Base and inheritance: the base theme, or "none". Follow the inheritance rules of `tokens.md`.
3. Token overrides: tokens that differ from the base.
4. Local roles: names that exist in this theme only (never portable names).
5. Palette definition: the stored `palette.py ramp` request and receipt. The receipt holds the schema version, method name and version, build id, output format version, `sha256` of the output, and the full normalized request with every default filled in. Regenerating from the receipt must give the same `sha256`. A normal build reads the stored palette and never regenerates it.
6. Component mappings: which component reads which semantic token.
7. Compatibility: modes, browsers, and the naming system in use (shadcn names or the colour grammar; 1 only).
8. Contrast evidence: 1 row per measured pair. A palette stop alone is not evidence.
9. Build artifacts: generated files, with their paths.
10. Verification map.
11. Decision log.
12. Open questions.
13. Scope boundary: what the theme does not cover.

### Contrast evidence rows

| Role | Ground | Mode | Stop | Ratio | Need |
|---|---|---|---|---|---|
| `<role>` | `<ground>` | `<mode>` | `<stop>` | `<n.nn>` | `<4.5 or 3>` |

Take each ratio from `palette.py check`, truncated to 2 decimals.

### Verification map

1 row per contract. Failure signal is what a person sees when the contract breaks.

| Contract | Evidence | Representative states | Failure signal |
|---|---|---|---|
| Palette identity | receipt `sha256` matches the stored output | n/a | `sha256` differs |
| Role mapping | every role resolves to a token | default, hover, disabled | missing token |
| Compatibility | build output in each mode | light, dark | missing variable |
| Contrast | `palette.py check` exits 0 | each mode | any `FAIL` line |
| Copy parity | `DESIGN.md` values equal the token file | n/a | a value differs |

### Decision log

1 entry per decision.

```markdown
### DEC-1
- Decider: <name>
- Date: <YYYY-MM-DD>
- Decision: <what was chosen>
- Rejected: <option> - <reason>; <option> - <reason>
```

## Template

```markdown
---
id: <theme-id>
kind: theme
status: current
approved_by: <name>
approved_on: <YYYY-MM-DD>
checked_by:
  - <path to test file>
recheck_when: [tokens, palette, component mappings, contrast, artifacts]
source: <path to token file>
references:
  - <path to token or inheritance record>
---

# Theme: <name>

## Intent and audience
## Base and inheritance
## Token overrides
## Local roles
## Palette definition
## Component mappings
## Compatibility
## Contrast evidence
## Build artifacts
## Verification map
## Decision log
## Open questions
## Scope boundary
```

## Rules

- Change the record and the token file in the same commit.
- Set `status: superseded` on the old record when a new one governs.
- A record without `approved_by` is a draft. Do not mark it `current`.
