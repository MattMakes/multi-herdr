# S5 skill-fixes report

Branch `ds/skill-fixes`. 5 items, 1 commit each (item 4 is committed before item 3, because the sort test also checks the README).

## 1. swiftdata-pro: @ModelActor

- Added `skills/swiftdata-pro/references/concurrency-and-actors.md` from `swiftdata-agent-skill` (`swiftdata-expert-skill/references/concurrency-and-actors.md`, revision `4819a538...`).
- Check against report §5: the file has no view-model or MV claim, so §5 "View architecture" does not apply.
- 2 edits:
  - The upstream example passed a `Trip` model into an actor method. The same file says not to pass model instances across isolation boundaries. The example now takes plain values (`name`, `startDate`).
  - 2 bullets added: `@concurrent` only for CPU-bound work (§5), and the CloudKit rules (§5, `cloudkit.md`) apply to a background writer.
- SKILL.md has a new review step and a reference entry.
- Provenance: new source entry with the upstream sha256 (`3b6a68e5...`); the edits are in `adaptation`.

## 2. swiftui-liquid-glass: symbol sizes

- 9 `.font(.system(size:))` uses on SF Symbol images (2 in SKILL.md, 7 in `references/liquid-glass.md`) became text styles: 32 and 36 to `.title2`, 30 to `.title`, 50 to `.largeTitle`.
- Fixed `.frame(width:height:)` values stay. They size the glass shape, not the text.
- SKILL.md was "verbatim" in provenance. It is now edited, and `adaptation` says so. Upstream sha256 values are unchanged.

## 3. provenance.json order

- All 85 entries sorted by name (byte order). The file keeps its format (`indent=2`, trailing newline), so the diff is a pure reorder.
- New test `skills_provenance_sorted_by_name` in `crates/horch-core/tests/skills_catalog.rs`.

## 4. README tables

- The ASC and audit tables sat under a second "Unreal Engine skills" heading, and the Swift section held 4 tables with 3 column layouts. Now there is one "Swift and Apple skills" table (28 rows, columns Skill, Upstream source, Kind) and one "Unreal Engine skills" table (33 rows, with `ue-build-verify` and `ue-editor-scripting`).
- The swiftdata-pro row lists the new reference.
- New test `skills_readme_tables_sorted_one_row_per_skill`: every table is sorted by first column (the "Source mapping" table keeps process order and is skipped), and the 85 skill rows match the 85 skill directories exactly once.
- Not sorted by a test: nothing else. The test is cheap, so no skip.

## 5. swift-security-expert: simulator and isAvailable

- Contradiction: `SKILL.md` Mistake #2 said `isAvailable` returns `false` on the Simulator. `references/secure-enclave.md` said it can return `true`, and its cross-validation note listed both claims.
- Apple's page (`developer.apple.com/documentation/cryptokit/secureenclave/isavailable`) says only "A Boolean value that indicates if the device supports Secure Enclave access". It does not mention the Simulator.
- Both files now say: the Simulator has no Secure Enclave, the value there is undocumented (usually `false`), and the compile-time `#if targetEnvironment(simulator)` guard stays. The provenance `adaptation` records the edit.

## Gotchas and follow-ups (outside scope, not fixed)

- `skills/swift-security-expert/references/testing-security-code.md` (lines 126 and 149) still says `isAvailable` returns `false` on every simulator. It contradicts the new wording a little. Fix it in a later unit.
- The vendored reference copy keeps the read-only mode of the source file. `cp` of a source file needs `chmod 644` before an edit.
