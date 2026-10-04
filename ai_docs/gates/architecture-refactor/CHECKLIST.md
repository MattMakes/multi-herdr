# Junior checklist for every phase (arch-refactor-dataset)

Status: provisional until the Spec A §16 text arrives.

Use this list for every commit and every phase of the arch-refactor-dataset
branch. Copy the lines that you checked into the commit body, unchanged. If a
line does not apply, copy it and add `(n/a: <reason>)`.

- [ ] `just gate` is green on this commit (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`).
- [ ] Every ID of this phase has a test (`scripts/check-req-coverage.sh --phase <P>` exits 0).
- [ ] Every new test name starts with its lowercase requirement ID (`ARC-02` -> `arc_02_...`).
- [ ] No golden prompt changed; any prose change is a named sanctioned block.
- [ ] No serialization golden re-blessed; a format change bumped its schema version.
- [ ] No oracle under `crates/*/tests/oracles/` regenerated.
- [ ] No existing test changed to make it pass, unless the unit plan says so.
- [ ] No new crate outside the allowlist (`scripts/check-deps.sh` and `nfr_05`, `nfr_06`, `nfr_09`, `nfr_11` pass).
- [ ] No new `std::env` read outside `runtime/` and the binary's bootstrap.
- [ ] No `ANTHROPIC_API_KEY` reaches a child process (`FORBIDDEN_ENV` strips it).
- [ ] Tests are hermetic: no network, no real harness binary, no herdr server, no file outside a temp dir; real `git` only on temp repos.
- [ ] Every moved module left a re-export shim (until A12).
- [ ] Old ledgers, old briefs and old teammate frontmatter still load.
- [ ] New `pub mod` lines in `crates/horch-core/src/lib.rs` are in alphabetical order; no other line changed.
- [ ] The diff was re-read adversarially; the unit report lists gotchas.
