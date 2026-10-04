# O1 operator-skills: Apple's Xcode skills on, without breaking other hosts

Branch `ds/operator-skills`. Plan: `ai_docs/plans/finish/o1-operator-skills.md`.

## What changed

- `SkillCatalog::with_operator_skills` (`crates/horch-core/src/skills/catalog.rs`)
  now skips a skill that this host does not have, and does not fail:
  - "Does not have" means: `dir` does not exist, or `<dir>/<name>` does
    not exist (`std::io::ErrorKind::NotFound` only).
  - Each skipped skill is recorded as a `SkippedOperatorSkill { name, path }`,
    read with `SkillCatalog::skipped_operator()`. `note()` gives the one
    sentence that the briefing and the warning use:
    `operator skill <name> is not installed on this host (no <dir>/<name>): ask the operator to run `xcrun agent skills export --output-dir <dir>` (Xcode 27 or later)`.
- These stay errors, on every host, also where the skill is missing:
  - an empty `names:`; a name given twice; an invalid skill id;
  - a name that a bundled or marketplace skill has (the clash);
  - `device-interaction` (a subagent skill by name);
  - `dir` that exists but is not a directory, or that `read_dir` cannot
    read (new: "operator_skills dir '...' cannot be read");
  - any stat error other than NotFound (for example a parent without
    search permission);
  - `<dir>/<name>/` that exists without a readable SKILL.md (new message:
    "operator skill '<name>' in '<dir>' has no readable <path>: <io error>");
  - an invalid SKILL.md, a SKILL.md that says "SUBAGENT skill" or
    "Agent tool", a tree the digest rules refuse.
  - The name checks now run before the filesystem checks, so a clash or
    `device-interaction` fails even on a host without the export.
- Briefing (`skills/briefing.rs`): after the expected and available lists,
  1 sentence per skipped skill: `Skipped: <note>.`
- `--check` (`roster/validation.rs`): new
  `pub fn operator_skill_warnings(roster) -> Vec<String>`, one line per
  skipped skill, `<teammate>: <note>`. `crates/horch/src/cmd/teammatescmd.rs:check`
  prints them as `warning:` lines after `fallback_warnings` (3 lines,
  approved by the orchestrator). Warnings never fail the check.
- `horch spawn` (`cmd/spawn.rs`) and the competition coordinator call
  `with_operator_skills` too. They did not change: they get the skip
  behaviour, and the ledger record lists only the copied skills.
- Teammates: `swift-developer` and `apple-platform-developer` now set
  `operator_skills: {dir: ~/.agents/skills, names: [swiftui-whats-new-27, test-modernizer]}`.
  The names are the 2 that `ai_docs/reports/swift-fleet-skills-2026-10.md`
  section 7.2 lists ("Start with `swiftui-whats-new-27` and `test-modernizer`").
  The comments give the export command and the reason `device-interaction`
  stays out.
- Docs:
  - `docs/recipes/add-teammate.md`: the operator_skills rules (warning vs
    error), the export command, and a fix of a stale line. The ledger record
    DOES list operator skills since `cmd/spawn.rs` extends the catalog, and
    `operator_skills_e2e_ledger_record_lists_them` pins it. The old text said
    it did not.
  - `README.md`: 2 new subsections under "Skills for each phase":
    "Apple's Xcode skills" and "App Store Connect key for app-release-preparer"
    (the second was an extra assignment from the orchestrator).

## The export command (evidence)

- Apple's documentation does not describe `xcrun agent skills export`. I
  searched developer.apple.com; "Giving external agents access to Xcode"
  covers only `xcrun mcpbridge`.
- 3 third-party guides agree on `xcrun agent skills export --output-dir <dir>`
  and the `--replace-existing` flag: sarunw.com
  (`/posts/export-xcode-agent-skills/`), onmyway133.com
  (`/posts/how-to-export-skills-from-xcode-27`), mehmetbaykar.com. 1 guide
  (bleepingswift.com) shows a positional `<dir>` and says the default is
  `~/.agents/skills`. The others do not name the default.
- Decision: the docs use the explicit form
  `xcrun agent skills export --output-dir ~/.agents/skills`, which works
  whatever the default is, and tell the operator to confirm with `--help`.
- Not verified on a host: this Mac has Xcode 26.6.

## App Store Connect key (evidence)

- Roles: Apple's role table (https://developer.apple.com/support/roles/,
  parsed from the HTML): "Submit apps" is Account Holder, Admin, App Manager.
  "Edit app pricing and availability" is Account Holder, Admin, App Manager,
  Marketing. "Upload builds" and "Manage TestFlight builds" include
  Developer. So the README recommends **Developer**. API key roles offered:
  Admin, App Manager, Developer, Marketing, Sales, Finance, Customer Support.
- The config schema is asc 5.9.1 `internal/config/config.go`: `Config`
  has `default_key_name` and `keys`; `Credential` has `name`, `key_id`,
  `issuer_id`, `private_key_path`.
- `asc auth status` (asc 5.9.1 `commands/auth.mdx`) shows the stored
  credentials and the default; `--validate` checks them over the network.
- The isolation sentence is from asc 5.9.1 `authentication.mdx`: "To
  isolate a CI job, sandbox, agent, or test run from stored config, set
  `ASC_CONFIG_PATH` to an absolute path ... and set `ASC_BYPASS_KEYCHAIN=1`."
- The README example has only `<KEY_ID>`, `<ISSUER_ID>` and `<you>`
  placeholders. No real or key-shaped value.

## Tests

- `crates/horch-core/tests/skills_catalog.rs`:
  - `operator_skills_check_errors` (changed, as the plan requires): a missing
    dir and a missing name are no longer problems. New asserts: a clash and
    `device-interaction` fail under a missing dir; a `<dir>/<name>/`
    without SKILL.md fails.
  - `operator_skills_missing_on_this_host_warn` (new): exact warning text;
    1 warning per name under a missing dir; none for a present skill.
  - `operator_skills_unreadable_dir_is_an_error` (new, unix): a mode 000
    dir fails `--check` with "cannot be read". It returns early under root.
  - `operator_skills_missing_on_this_host_are_skipped_with_a_briefing_note`
    (new): the bundle holds the present skill, not the missing one; the
    briefing has the exact `Skipped:` sentence; a missing dir still launches.
- `crates/horch-e2e/tests/skills_exposure.rs`:
  `operator_skills_e2e_check_fails_on_a_missing_name` is now
  `operator_skills_e2e_check_warns_on_a_missing_name`: `--check` passes with
  a `warning:` line, the launch lists only `tdd`, and the Claude prompt has
  the `Skipped:` sentence.
- All tests use a temp operator dir. None reads the real `~/.agents/skills`.

## Gotchas

- `operator_skills_check_errors` takes about 5 minutes in a debug build
  (each `check_problems` call checks the whole repo roster). It was slow
  before; this unit adds 4 calls.
- A teammate whose only skills are operator skills, all missing, gets no
  bundle (`Bundle::install_from` returns `None` when nothing is activated),
  so no briefing paragraph and no `Skipped:` note. `--check` still warns.
  No shipped teammate is in this case.
- On this Mac, `~/.agents/skills` exists with unrelated skills, so
  `horch teammates --check` prints 4 warnings (2 teammates x 2 names) until
  the operator exports the Xcode 27 skills.

## Follow-ups (not done, outside this unit's files)

- `swift-qa-engineer` could also take `test-modernizer`; the report section
  7.2 names only "the developers".
- `horch teammates --matrix` still does not show `operator_skills`.
