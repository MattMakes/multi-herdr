# Post-flight: phase skills

PASS for the implemented macOS/Linux integration, with the platform and environment limits below.

## Plan conformance

- Fourteen repo-owned skill adaptations are embedded in the binary with provenance. No runtime source checkout, package download or personal skill path is required.
- Research, plan, implementation and validation catalogs resolve; explicit additions deduplicate. The CLI shows catalog metadata cost and deferred skill file sizes.
- Fresh/resumed fleet workers and fixed recipe panes use native adapters. Phase persists in the ledger and resolved brief. Existing records remain readable.
- Bundles are unique per launch and cleaned on exit/error. Codex auth/config stay linked to original state; its selected skill directory and execpolicy remain private. Original user skill contents survive cleanup.
- OpenCode merges the inline skill-path overlay without removing existing providers, denials, paths or URLs. Claude preserves explicit settings and permission denials. pi/Prime explicitly load selected skills under --no-skills.
- Codex disables update checks and resume-directory prompts, handles enabled hook trust per invocation, and bundled roles use workspace-write with approval policy never. No approvals-and-sandbox bypass is introduced.

## Verification

- `cargo test --workspace`: 248 tests passed (40 docs-sync, 37 installer, 33 CLI, 133 core, 5 golden prompt tests); no failures.
- G1–G4 in `ai_docs/gates/phase-skills/GATES.md`: all met after final source edits.
- `cargo build --release -p horch`: exit 0.
- `target/release/horch teammates --check`: 22 teammates, 17 offered; exit 0.
- `cargo clippy --workspace --all-targets`: exit 0 with existing warnings in prior code. New warnings from this change were corrected.
- `git diff --check`: exit 0. Changed Rust files formatted with rustfmt.
- Native skill discovery/control checks: Claude Code 2.1.274, Codex 0.154.0, OpenCode 1.18.2, pi 0.85.1, Prime Agent 0.9.4. See `docs/runtime-skill-checks.md` for procedures, observations and scope. No model inference requests were made.
- Independent implementation and integration reviews completed. Confirmed late-validation and Windows shared-write-before-error findings were fixed. Prime daemon argument ordering was also corrected and regression-tested.

## Limits

- Full authenticated task execution across five provider models was not exercised; catalog/runtime initialization and launch contracts were verified.
- Ambient project/user skill discovery can remain visible in Claude, Codex and OpenCode. Estimates describe the fleet catalog, not total model request tokens.
- Native Windows Codex phase skills require WSL. Unsupported selections fail before pane allocation/shared rule writes; legacy custom teammates can opt out of phase skills. Native Windows runtime tests were not run.
- pi 0.85.1 requires Node >=22.19.0. Default Node 22.9.0 fails before pi CLI parsing; installed Node 25.5.0 successfully ran its CLI version check. No global runtime/PATH changes were made.
- The branch includes pre-existing OpenCode/pi/Prime harness additions; an older installed horch still needs rebuilding/reinstalling to use this branch. The just recipes run this checkout through Cargo.
