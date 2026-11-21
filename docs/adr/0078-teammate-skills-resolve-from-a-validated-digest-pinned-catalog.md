# Teammate skills resolve from a validated, digest-pinned catalog

A teammate's skills come from a `SkillCatalog`
(`crates/horch-core/src/skills/catalog.rs`): the compiled-in skills, the
skills pinned in `marketplace.lock`, and the operator and plugin skills
the teammate brings from the operator's machine. `SkillResolution`
(`skills/resolution.rs`) reads that catalog once and plans which skills
activate. `skills/materialize.rs` copies them into the launch's bundle and
checks each copy against the pinned digest. The briefing
(`skills/briefing.rs`) tells the worker which skills it is expected to
use.

## Consequences

- `SkillCatalog::bundled` builds the compiled-in catalog once per process
  in a `OnceLock`. The files are compiled in, and digesting them on every
  call made `Roster::check` (1 call per teammate) slow. A clone is cheap,
  because the entries hold `&'static` file bytes.
- Skill ids and versions read from `marketplace.lock` are checked as plain
  path segments before they are joined into store paths. The id follows
  the SKILL.md name rules (`SkillId::parse`), because an id later names a
  directory. `version_dir` builds `<root>/skills/<id>/<version>/` only
  after checking that `id` and `version` are plain path segments (no
  empty, `.` or `..` version): the lock is an operator-writable file.
- `SkillCatalog::with_host_skills` adds the teammate's `operator_skills:`
  and its `plugin_skills:` (`SkillCatalog::with_plugin_skills`). `~/`
  expands against `home`, the launch's home. Only `SkillResolution::read`
  calls it, for the spawn, `horch fleet`, the competition coordinator, the
  launch and `--check`, so the ledger record and the launch's skill check
  (SKL-04) see the same skills.
- An operator skill this host does not have (no directory, or no
  `<dir>/<name>/`) is skipped and recorded in
  `SkillCatalog::skipped_operator`. The skills are exported per host with
  Xcode (`xcrun agent skills export`), so 1 host may lack them. The name
  rules apply on every host, so a clash fails even where the skill is
  missing. A teammate without the field changes nothing.
- `with_host_skills` fails on a name that a catalog skill already has (1
  bundle directory cannot hold both), a name given twice, a subagent
  skill, a directory or skill that exists but cannot be read, an invalid
  SKILL.md, or a tree the marketplace digest rules refuse (a symlink, for
  example).
- `SkillResolution::plan` takes the teammate that will launch (after
  `Roster::for_launch`), the one `SkillResolution::read` read. It fails on
  an unknown skill in `skills:` or the phase, on skills the teammate
  cannot load, and, when the plan activates any skill (operator and plugin
  skills count), on the support check, which is the rule the launch
  applies.
- An `available_skills:` name the catalog lacks is dropped with 1 `NOTE:`
  line on stderr. A teammate file can name a skill that this horch build
  does not bundle yet, and an offer must not refuse a spawn.
- `materialize::copy_verified` checks the tree digest of the copy, not the
  source, so what the launch reads is exactly what the lock (or, for an
  operator skill, the activation plan) pinned.
- `orchestrate` and `skill-creator` are attached by name, to the
  orchestrator teammates only, and belong to no phase catalog. Adding
  either to `Phase::Plan` would hand it to `staff-engineer` and every
  other plan-phase teammate, the opposite of what it is for.
- `skills::briefing::render` names the teammate's own `skills:` and its
  `plugin_skills` as EXPECTED, each with its description, so the worker
  knows when its step has come. A bare list of names read as optional,
  and workers skipped them. The rest of the phase catalog is listed by
  name only, which keeps the context cost of a broad phase small.
- `Bundle::briefing_in` ignores errors from
  `claude_plugins::resolve_all_in` when it lists plugin skill
  descriptions. Plugin skills exist only where skills load as plugins. A
  plugin that does not resolve is reported by `horch teammates --check`,
  and the briefing must not fail a launch over a description.
