# Phase Skills Implementation Plan

**Goal:** Portable phase-scoped skills in all five fleet harnesses, pushed to the existing PR.
**Architecture:** Embedded curated skill assets are materialized per launch and connected to native harness skill discovery. Defaulted teammate/ledger phase supports per-task selection and resume.
**Gates:** ai_docs/gates/phase-skills/GATES.md
**Tech Stack:** Rust, serde YAML/JSON, Markdown Agent Skills.
**Wiring Manifest:** assets -> build.rs -> skills.rs -> launch::command_with_skills -> worker and recipes. CLI --phase -> Teammate.phase -> Brief.resolved and ledger phase -> resumed teammate.
**Regression Hotspots:** launch.rs prompt ordering, codex.rs private-home symlinks, worker.rs cleanup, mailbox.rs serialized teammate, ledger.rs old records, prompts.rs golden fixtures.

## Task 1 [x]: portable skill content
OWNS: skills/**
Gates: G1
Read the source skills under ~/projects/public-skills. Create self-contained curated adaptations, using common Agent Skills frontmatter; retain provenance (source revision and per-skill source paths). Names: brainstorm, research-codebase, trace, create-plan, pre-flight, execute, tdd, debug, check, code-analysis, code-review, security-review, handoff, document. Keep bodies concise and dependencies self-contained. Avoid Claude-only tools, fixed model names, human checkpoint waits when already authorized, and paths outside the bundle. Document deliberate adaptation, especially code-analysis using project-native analyzers rather than copying the upstream large JS engine. No runtime downloads. Validate YAML and relative links.

## Task 2 [x]: phase contracts, CLI and roster
OWNS: crates/horch-core/src/teammates.rs, crates/horch-core/src/ledger.rs, crates/horch/src/main.rs, crates/horch/src/cmd/spawn.rs, teammates/**
Gates: G3
Add Phase enum (research, plan, implementation, validation) in teammates.rs with serde snake_case, FromStr/Display; Teammate.phase Option<Phase> default None. Expose --phase on spawn, override teammate default for fresh sessions, preserve/override ledger phase on resume. Persist optional phase on records with backward compatibility (new method if needed). Brief.resolved already persists teammate. Builtin defaults: researchers/product-lead/designer research, staff-engineer plan, architect-reviewer/qa-engineer validation, implementation workers implementation, orchestrators plan; smoke/no-agent no phase. Replace local plugin_dirs and external skills declarations in bundled roster with relevant available bundled names; preserve MCP and tool settings. Remove false unsupported Codex skills assertion. Retain explicit custom Claude plugin_dirs support. Validate phases and recognized bundled skill names at appropriate boundary. Regression tests first. Coordinate API with root.

## Task 3 [x]: native integration and verification
OWNS: crates/horch-core/src/skills.rs, crates/horch-core/build.rs, crates/horch-core/src/lib.rs, crates/horch-core/src/launch.rs, crates/horch-core/src/codex.rs, crates/horch-core/src/prompts.rs, crates/horch-core/tests/**, crates/horch/src/cmd/worker.rs, crates/horch/src/cmd/recipes.rs, README.md, docs/**, ai_docs/**
Gates: G1, G2, G4
Write failing adapter tests. Embed assets recursively at build; create unique per-launch bundle with Drop cleanup and no shared writes. Add command_with_skills API, preserve command existing tests. Wire fresh/resumed workers AND fixed recipe panes. Codex: install native skills into private CODEX_HOME without following shared symlinks. Claude: generated plugin plus low-context settings preserving operator config. OpenCode: merge OPENCODE_CONFIG_CONTENT without discarding provider/settings. pi/Prime explicit skill flags. Add short phase-aware prompt guidance, never paste skill bodies; correct old eager-load instructions. Document supported versions, native discovery mechanisms, context measurement and integration limits with official citations. Run workspace tests, roster validation, build and review. Commit explicit changed paths, push branch, rewrite PR #1 title/body around the complete diff.

## Plan Verification Checklist
- Pre-flight: standalone CLI (no server setup needed); assets have no auth; harness credentials stay where they are. No new API credentials.
- Verify all preservation-analysis rows in the design against the final implementation.
- Post-flight: rerun acceptance checks after fixes; do not infer runtime discovery from argv tests alone. Report unavailable harness smoke checks explicitly.
