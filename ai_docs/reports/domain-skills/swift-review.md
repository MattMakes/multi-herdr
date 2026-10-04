# S3 swift-review: review, test, accessibility and security skills

Unit: `swift-review`. Branch: `ds/swift-review`. Worker: opus-43. Date: 2026-10-03.
Plan: `ai_docs/plans/domain-skills/s3-swift-review.md`. Specification:
`ai_docs/reports/swift-fleet-skills-2026-10.md` §4.3, §5, §7.3.

## Result

9 skills under `skills/`. Each has an
entry in `skills/provenance.json` (sha256 of every copied upstream file, 40-hex pin
from `PINS.txt`), and a row in the new "Swift and Apple skills"
table in `skills/README.md`. `./target/debug/horch skills` lists all 9.
No skill is in a `Phase`. No Rust code or teammate changed.

| skill | upstream | verdict applied | SKILL.md bytes | dir bytes | vendored |
|---|---|---|---|---|---|
| appkit-accessibility-auditor | apple-accessibility-skills | Adapt, path only | 11,482 | 16,408 | no |
| ios-simulator-run | Skills `ios-debugger-agent` | Adapt into a playbook | 3,198 | 4,270 | no |
| swift-code-audit | ios-code-audit | Adapt, heavily | 9,360 | 27,271 | no |
| swift-security-expert | swift-security-skill | Adapt (cut 36%) | 30,899 | 482,666 | yes |
| swiftdata-testing | ios-swiftdata-testing-agent-skill | Adapt | 4,277 | 21,818 | no |
| swiftui-accessibility-auditor | apple-accessibility-skills | Adapt, path only | 7,352 | 11,866 | no |
| swiftui-performance-audit | Skills | Adapt | 5,446 | 20,790 | no |
| uikit-accessibility-auditor | apple-accessibility-skills | Adapt, path only | 11,596 | 16,413 | no |
| writing-for-interfaces | skills | Adapt | 12,736 | 26,620 | yes |

"Vendored" follows the plan rule: the references are
verbatim and the skill is over the 12 KB / 160 KB budget.

## Gate

**Final:** after the rebase on `design-skills` at `27d2df6` (V0 merged), `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`
prints `GATE GREEN`. The rebase conflicted only in `skills/README.md` (both sides kept) and
`skills/provenance.json` (the `design-skills` file plus my 9 entries in name order; V0's
`skill-creator` change kept).

Before V0 merged, the gate failed only on the 3 V0 items,
all in `crates/horch-core/tests/skills_catalog.rs`:

- `skills_bundled_size_budget`: `swift-security-expert/SKILL.md: 30899 bytes` (V0 step 3 exempts `vendored: true`).
- `skl_01_bundled_catalog_versions_and_digests`: `apple-accessibility-skills ... is not a pinned upstream` (V0 step 4 checks the shape instead).

Every other test passes (`cargo test --workspace --no-fail-fast`), and `cargo fmt --check`,
both builds, `horch teammates --check`, `check-req-coverage.sh`, `check-deps.sh` and
`verify-telemetry-e2e.sh` pass. Because each failing test stops at its first hit, I
checked the post-V0 rules with a script over all 9 skills: only `.md` files; the 7 non-vendored skills are inside 12 KB / 160 KB; the 2 over budget
carry `vendored: true`; every source has a `https://github.com/<o>/<r>` repository, a
40-hex revision, and a 64-hex sha256; every description is at most 1024
bytes (largest: 466). The `vendored` field parses today because provenance parsing
ignores unknown fields; `horch skills` loads the catalog without error.

## Per-skill edits and evidence

### Step 1: the three apple-accessibility-skills auditors (path fix only)

- `swiftui-accessibility-auditor/SKILL.md:132`, `uikit-accessibility-auditor/SKILL.md:194`,
  `appkit-accessibility-auditor/SKILL.md:195`: `skills/<name>/checklist.md` becomes `checklist.md`.
- `checklist.md` and the frontmatter (`version`, `compatibility`) are verbatim. The loader
  accepts extra frontmatter keys (`Metadata` in `crates/horch-core/src/skills/catalog.rs`
  reads only `name` and `description`).
- No ask gates, subagents or `${CLAUDE_*}` variables in the sources (grep checked).
- **Skipped:** `swift-accessibility-skill`'s `nutrition-labels.md` / `performAccessibilityAudit`
  material. `PasqualeVittoriosi/swift-accessibility-skill` is not in
  `.worktrees/_sources/swift/PINS.txt`, so it is not a pinned source. The report's §4.3 row
  asks for it; a later unit can add it after the source is pinned.

### Step 2: swiftui-performance-audit

- `SKILL.md:3` description: "guidance for user-run Instruments profiling" becomes "record and read an Instruments trace".
- `SKILL.md:10,16,17,28`: the ask-the-user intake becomes "record it yourself" or "message the orchestrator".
- `SKILL.md:54-62` (was "3. Guide the User to Profile"): the worker runs
  `xcrun xctrace record --template SwiftUI --device <name|UDID> --time-limit 15s --output ... --launch -- <app>`
  (or `--attach`), reads it with `xcrun xctrace export --input <trace> --toc` and `--xpath`, and
  messages the orchestrator if it cannot record.
- `SKILL.md:87` (was "Ask the user to re-run"): re-record and compare with the baseline.
- Dropped upstream `SKILL.md:102` ("Add Apple documentation ... under `references/` as they are
  supplied by the user"): a bundled skill must not edit itself.
- `references/profiling-intake.md:5,7,16-21,23,31-35`: the same change in the intake checklist.
- `references/code-smells.md:19-25`: the cached shared `DistanceFormatter` becomes
  `Text(Measurement(...), format: .measurement(width: .abbreviated))` (§5 fleet answer: FormatStyle for display).
- Evidence: on this host `xcrun xctrace version` prints `xctrace version 16.0 (17F113)`;
  `xcrun xctrace list templates` lists `SwiftUI`; `xcrun xctrace help record` documents
  `--template`, `--device`, `--time-limit`, `--output`, `--attach`, `--launch --`.
- Other 5 references verbatim.

### Step 3: swift-security-expert

- `SKILL.md` cut from 48,408 to 30,899 bytes (36%, the report asks about 40%):
  - Removed "Scope Boundaries — Inclusions" and "Exclusions" (upstream `:271-305`); replaced by one scope paragraph at `SKILL.md:9`.
  - Removed "Tone Rules" (upstream `:309-330`), including its duplicate 10-item directive list.
  - Removed "Agent Self-Review Checklist" (upstream `:454-470`).
  - Removed the ASCII decision tree (upstream `:21-48`); its 3 branches remain as prose; `SKILL.md:15` names them.
  - Removed output rule 10 (upstream `:406`): it told the agent to cite the removed sections.
  - Removed markdown table cell padding (no content change).
  - `SKILL.md:15`: "If ambiguous, ask" becomes "message the orchestrator".
  - `SKILL.md:235`: new "## Agent Rules" heading, so the kept rule sections are not under "Authoritative Sources".
- Kept: core guidelines (biometrics bound to `SecAccessControl`, add-or-update on
  `errSecDuplicateItem`), quick-reference tables, ML-KEM / ML-DSA availability, review
  checklist, 10 common AI mistakes, loading rules, output and behavioral rules.
- **Decision on the 3 optional references: keep all 3 verbatim.** Reasons:
  - References load on demand, so they cost a pane nothing per turn. The only cost is about 108 KB in the binary.
  - Dropping any one leaves dangling cross-references in verbatim files:
    `compliance-owasp-mapping.md` is named in 6 other references,
    `testing-security-code.md` in 6, `migration-legacy-stores.md` in 8. Fixing them means
    editing references, which loses the verbatim (vendored) status.
  - `migration-legacy-stores.md` backs the most common fix a reviewer gives (UserDefaults to Keychain), and Mistake #10 cites it.
  - `testing-security-code.md` has the keychain-specific simulator, device and CI limits that `swift-qa-engineer` needs.
  - `compliance-owasp-mapping.md` loads only when a task names OWASP or an audit (`SKILL.md` loading order step 4).
- Vendored: over budget. The references are verbatim except for key-shaped strings (next item).
- **Key-shaped strings removed (orchestrator REBASE, 2026-10-03).** GitHub push protection
  rejected `design-skills`: secret scanning read an upstream example Stripe key as a live key.
  No key-shaped string may enter any commit, so I rewrote `ds/swift-review` (the first skill
  commit was amended, so no commit holds the strings). Each example keeps its lesson:
  - `references/compliance-owasp-mapping.md:118`, `references/credential-storage-patterns.md:39-40`,
    `references/common-anti-patterns.md:91-92`: the hard-coded example Stripe and Firebase keys
    become `"<STRIPE_LIVE_SECRET_KEY>"` and `"<FIREBASE_API_KEY>"`. They are still string
    literals in source, which is the anti-pattern shown.
  - `references/common-anti-patterns.md:144`: the detection grep becomes
    `grep -rnE '"[sp]k_live_|"AIz[a][A-Za-z0-9]|"AK[I]A[A-Z0-9]'`. It matches the same strings
    as the upstream BRE, but its text holds none of the prefixes.
  - `references/cryptokit-public-key.md:324`: the three PEM header lines become PEM labels
    (`PRIVATE KEY`, `PUBLIC KEY`, `EC PRIVATE KEY`).
  - The provenance `adaptation` records these edits. The sha256 values still hash the upstream files.
  - Check: the orchestrator's 7-pattern key grep (Stripe live and test, AWS, GitHub token,
    Google API key, Slack token, PEM private key header) finds nothing in the 9 skills or in
    `git log -p` of this branch.

### Step 4: swift-code-audit (from ios-code-audit)

- Renamed: `name: swift-code-audit`, new 346-byte description without "the user".
- Three parallel Explore agents (upstream `SKILL.md:58-66`) become three sequential passes
  the worker runs itself: `SKILL.md:50-58`. `references/agent-prompts.md` becomes
  `references/pass-checklists.md` (persona lines and fences removed, "Agent" becomes "Pass",
  the "single message with multiple `Agent` tool calls" line removed).
- Report path: `SKILL.md:10` — the path the orchestrator gives; never `CODE_AUDIT.md` in the
  repository root. Upstream "Skill output" section (`:153-155`) removed.
  `references/report-template.md:1-3` points to that path; the "ping me" line becomes "send the
  reference back for re-investigation".
- Dropped the `swiftui-expert-skill` step (upstream `:68-79`). Its checks (`@State` /
  `@Bindable` / `@Observable` misuse, modifier order, accessibility gaps, `Equatable` leaves)
  move into Pass C item 4 so report §8 still gets findings. The upstream pointer for profiling
  becomes the bundled `swiftui-performance-audit` (`SKILL.md:140`).
- `mcp__xcode__XcodeListNavigatorIssues` (Xcode's MCP bridge, which §7.1 leaves out) becomes
  "the XcodeBuildMCP build tool if connected, else `xcodebuild`" (`SKILL.md:41`). A build that
  fails goes to the orchestrator (`SKILL.md:48`).
- `CLAUDE.md` mentions become "the project's agent docs (`AGENTS.md`, `CLAUDE.md`)".
- `SKILL.md:131`: on completion, message the orchestrator with path, counts and top 3 items.
- Kept the 4 named parts: compiler warnings as ground truth (Step 2), verify every Critical
  claim (rule and Step 4), group by root cause, and the concurrency and API-modernity checklist
  (Pass A, verbatim). Also kept the 12 numbered sections and severity guide. The upstream's
  "reports without numbers will be rejected" repetition is shortened to one statement.

### Step 5: swiftdata-testing

- **Checked fix, `references/decimal-money-values.md:7-33`** (upstream `:8-23`):
  - A fractional literal is not exact: `Decimal`'s float-literal initializer takes a `Double`.
    The replacement uses integer literals, `Decimal(sign:exponent:significand:)` and
    `Decimal(string:locale:)` with `en_US_POSIX`.
  - `Decimal` division is not exact: the split test now says that `splitEvenly` must round to
    the minor unit and give the remainder to one part, and asserts the parts are 33.34, 33.33, 33.33.
  - Matching rules fixed at `:38` (upstream told the reader to write `19.99` as a `Decimal`
    literal) and `:39` (upstream said the type guarantees the re-sum).
  - Evidence: a Swift script run on this host (`swift`, Xcode 16 toolchain) printed:
    `literal 3.133 -> 3.132999999999999488`; `literal 19.99 -> 19.98999999999999488 false`;
    `sig -> 19.99 true`; `100/3 -> 33.333333333333333333333333333333333333 x3 sum -> 99.999999999999999999999999999999999999 false`;
    `[33.34, 33.33, 33.33] true`. The macOS SDK's `Foundation.swiftmodule/arm64e-apple-macos.swiftinterface`
    declares `extension Foundation.Decimal : Swift.ExpressibleByFloatLiteral { public init(floatLiteral value: Swift.Double) }`.
    Apple page: https://developer.apple.com/documentation/foundation/decimal/init(floatliteral:)
    (it renders only with JavaScript, so the SDK interface is the evidence).
- **MVVM conditional (§5):** `SKILL.md:15` — follow the codebase; mock repositories only when view
  models with repository protocols already exist; in MV, test model logic directly with the
  in-memory fixtures. `references/mock-repositories.md:3` gets the same scope note.
- `SKILL.md:32,40`: "the user asks" becomes "the task asks".
- Other 3 references verbatim.

### Step 6: ios-simulator-run (from ios-debugger-agent)

- Renamed; new description.
- Neutral tool names: the `mcp__XcodeBuildMCP__` prefix is gone; each step names the job and
  the bare tool name, and tells the worker to match the job to the server's tool list because
  versions rename tools (`SKILL.md:8`).
- Boots a simulator itself (upstream `:16` "do not boot automatically"): `SKILL.md:15`, with
  `boot_sim` or `xcrun simctl boot` + `xcrun simctl bootstatus <UDID> -b`. Never erase or delete a simulator.
- Failures go to the orchestrator instead of the user (upstream `:27`, `:49`): `SKILL.md:29,52`.
- `xcrun simctl` fallbacks for list, boot, launch and log stream. Evidence: `xcrun simctl help`,
  `xcrun simctl help bootstatus` and `xcrun simctl help spawn` on this host document each command used.
- Not verified: the XcodeBuildMCP tool names. XcodeBuildMCP is not installed on this host. The
  GitHub README (fetched 2026-10-03) shows the project now presents itself as "MobileBuildMCP"
  and its docs page names only `build_run_sim` and `launch_app_sim` in text. The other names come
  from the pinned upstream skill. The P-Swift persona unit should confirm them on the installed version.

### Step 7: writing-for-interfaces

- Removed `context: fork` (upstream `:14`).
- Description: 828 characters (folded) to 215, without "trigger whenever" (`SKILL.md:3`).
- Voice interview (upstream `:48-74`) replaced at `SKILL.md:37-50`: infer the voice from the
  codebase's existing copy; if too thin, message the orchestrator with the 3 questions and
  continue with provisional neutral copy; put the voice used at the top of the result. The
  upstream "suggest the user persist it in AGENTS.md" step is replaced by that last line.
- SKILL.md is 12,736 bytes, 448 bytes over 12 KB with only the report's edits. I did not cut
  more prose; the references are verbatim, so the skill is `vendored: true` per the plan rule.
  If the orchestrator prefers it inside budget, removing the "The simplest test" section
  (about 330 bytes) and the closing "Patterns reference" pointer (about 280 bytes) is enough.

## Not done and follow-ups

- `nutrition-labels.md` / `performAccessibilityAudit` material: skipped, source not pinned (see Step 1).
- XcodeBuildMCP tool names unverified (see Step 6). The README rename to MobileBuildMCP may
  also affect the launch arguments in report §7.1.
- Outside scope, noticed: `swift-security-expert/references/secure-enclave.md:68` says simulator
  `SecureEnclave.isAvailable` behavior varies, while SKILL.md Mistake #2 says it returns `false`.
  Left verbatim.
- The first commit carries provenance entries for 4 skills that the second commit adds; the
  branch head is consistent.
