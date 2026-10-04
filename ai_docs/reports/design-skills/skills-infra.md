# D00 skills-infra report

Branch `ds/skills-infra`. The gate is green.

## Provenance JSON shape

A skill entry in `skills/provenance.json` lists one item per adapted file:

```json
{
  "name": "ui-taste",
  "sources": [
    {
      "repository": "https://github.com/example/design-skills",
      "revision": "0123456789abcdef0123456789abcdef01234567",
      "path": "skills/taste/SKILL.md",
      "sha256": "<64 hex digits of shasum -a 256 on the original file>"
    }
  ],
  "adaptation": "One line: what we merged, rewrote and dropped."
}
```

- A repo-original skill uses `"sources": []` and an `adaptation` text.
- The old single-source fields still parse as a 1-item list.
- Mixing `sources` with the old fields is an error.
- A `sha256` that is not 64 hex digits is an error.
- A duplicate skill name is an error.
- Add entries in alphabetical order by skill id.

## Decisions

- `Provenance` is now `{sources: Vec<SourceRef>, adaptation}`. The old public fields are gone. The only caller was `skills show`.
- `parse_provenance` in `catalog.rs` is public so tests can parse a fixture.
- `orchestrate` now has a `Provenance` with no sources. Before, it had `None`. The JSON entry is unchanged.
- An old entry with `source_path` but no `source_sha256` is now an error. Before, it gave `None`. No bundled entry is affected.
- `skills show` prints one `upstream:` line per source and one `adaptation:` line. `--json` prints the new `provenance` shape.
- `EXEMPT_FROM_BUDGET` holds `skill-creator` only. Its SKILL.md is 33168 bytes, its directory is 248 KB, and it has `.py`, `.html` and `.txt` files. The other 15 skills meet the budget.
- The catalog test now compares the set of skill directories with the catalog. The count 16 is gone.

## Follow-ups

- The README "Source mapping" table and the intro text still describe one upstream. Skill units add rows to "Design skills".
