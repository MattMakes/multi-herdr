# claude.ai-synced skills in fleet panes: research (Claude Code 2.1.276)

Brief: `ai_docs/plans/disable-claudeai-synced-skills.md`, step 1.
Binary: `/Users/mascott/.local/share/claude/versions/2.1.276` (Mach-O arm64).
Method: `strings` on the binary, then a live A/B launch.

## 1a. A single switch exists: `syncClaudeAiSkills: false`

Claude Code 2.1.276 has one settings key that turns off all claude.ai-synced
skills at once. The key is `syncClaudeAiSkills`. The settings schema describes it
as follows (verbatim):

> Set to false to turn off syncing of the skills you have enabled on claude.ai.
> In your user settings (or managed settings): nothing more is downloaded,
> previously synced skills (~/.claude/skills/synced) can no longer be run, are
> hidden from every session started afterwards, and are moved to
> ~/.claude/skills/.trash at the next launch (deleted after cleanupPeriodDays;
> re-downloaded, not restored, if you re-enable). In .claude/settings.local.json
> or --settings: downloads stop and synced skills are blocked and hidden for
> sessions in that workspace or invocation only (nothing is moved). Not read from
> project settings (.claude/settings.json). Only false is honored [...] Only
> applies when signed in with your Claude account.

What the code does:
- The sync gate is `jke = {settingKey:"syncClaudeAiSkills", policyKey:"allow_account_skills_sync", flagName:"tengu_account_skills_sync_enabled"}`.
- `lKt(key)` returns false if any source sets the key to `false`. The sources
  are `policySettings`, `flagSettings` (`--settings`), `userSettings`,
  `localSettings`, and managed settings. A `false` in `--settings` therefore
  vetoes the sync for that session.
- The trash move is in `Uhn(jke)`. It reads only managed settings and
  `userSettings`, not `flagSettings`. A `false` in `--settings` does not move
  or delete files.
- Only `false` is honored. An invalid value is treated as `false`.

A sibling key `syncClaudeAiPlugins` works the same way for synced plugins. The
operator's `disableClaudeAiConnectors: true` is the MCP connector switch, not a
skill switch. No env var turns the skill sync off. `CLAUDE_CODE_SYNC_SKILLS`
only forces the sync on for a feature check (`Gte()`).

Consequence for the brief: step 2 takes the global-switch branch. horch emits
`"syncClaudeAiSkills": false` in its `--settings` overlay. It does not need a
discovered list of synced skill names.

## 1b. Local cache

Path pattern:

    ~/.claude/skills/synced/<account-uuid>_<org-uuid>/<skill-name>/SKILL.md
    ~/.claude/skills/synced/<account-uuid>_<org-uuid>/manifest.json
    ~/.claude/skills/synced/.bucket-<account-uuid>_<org-uuid>   (empty marker)

On this machine the bucket directory holds the 11 skills: anki-learning-builder,
docs, docx, import-memory, morning, pdf, persona, pptx, skill-creator,
treasures-talk-writer, xlsx. `manifest.json` has `lastUpdated`, `skills`, and
`pendingClaims`. Each entry in `skills` has `skillId`, `name`, `description`,
`source` (`anthropic-example` or `custom`), and `updatedAt`.

Name mapping: the listed name is `anthropic-skills:<name>`, where `<name>` is the
directory name and the manifest `name`. The binary has this rule: "synced
skill(s) answer only to anthropic-skills:<name> this session because their bare
names cannot be checked for collisions".

The global switch makes this cache irrelevant to horch. horch does not read it.

## 1c. `skillOverrides`

The schema is `skillOverrides: record<string, "on" | "name-only" |
"user-invocable-only" | "off">`. The description is: "Per-skill listing
overrides keyed by skill name. [...] "off" hides it from both. Absent = on."
The value that turns a skill off is `"off"`.

The synced-skills switch does not need `skillOverrides`. horch emits
`skillOverrides` only for a teammate's `disabled_skills` entries (see
"Decision and implementation"). A later live check shows that those entries
merge per key with the operator's `{"explain-diff-notion": "off"}`.

## Live A/B check

Command, run from the repo root with `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`
and `< /dev/null`:

    claude --model sonnet -p 'Reply OK' --output-format stream-json --verbose \
      --settings '<overlay>'

The skill list is from the stream-json `system/init` message.

| Overlay | skills | slash_commands | anthropic-skills entries | explain-diff-notion |
|---|---|---|---|---|
| `{"disableBundledSkills":true,"disableWorkflows":true}` (what horch sends today) | 35 | 65 | 11 | absent (off) |
| same plus `"syncClaudeAiSkills":false` | 24 | 54 | 0 | absent (off) |

The switch removes exactly the 11 synced skills. It changes nothing else in the
list. No settings error was printed.

Note: this shell exports an `ANTHROPIC_API_KEY` that the API rejects (401). The
init message is emitted before the API call, so the skill lists above are valid.
The step-5 check therefore runs with `env -u ANTHROPIC_API_KEY`, so that Claude
Code uses the claude.ai login, as the fleet panes do.

## Finding against the brief: not every Claude teammate has `inherit_plugins: false`

The brief states that every Claude teammate has `inherit_plugins: false`. That is
not true. These Claude teammates use the default `inherit_plugins: true`:
- `teammates/opus.md`
- `teammates/sonnet.md`
- `teammates/orchestration-orchestrator.md`
- `teammates/orchestration-worker.md`

A live `ps` confirms it. The opus-1 pane runs with
`--settings {"disableBundledSkills":true,"disableWorkflows":true}` and no
`enabledPlugins` map. If the switch depended on `inherit_plugins: false`, the
opus and sonnet tiers would keep the synced skills. That breaks the goal "OFF
by default in every Claude pane horch launches".

## Finding against the brief: two overlay builders

Fleet panes launch with a skill bundle. The bundle path does not use the
overlay block in `launch.rs`. `skills.rs` `Bundle::claude_settings` builds the
JSON and sets it as `teammate.settings`. `launch.rs` then passes that JSON
through verbatim. The switch must be in both builders, or it has no effect in
the fleet.

## Decision and implementation

The operator approved these changes to the brief:
- horch emits `syncClaudeAiSkills: false` in every Claude pane unless the
  teammate sets `inherit_claudeai_skills: true`. The switch does not depend on
  `inherit_plugins`.
- horch keeps the teammate list field `disabled_skills`. Each entry becomes a
  `skillOverrides` entry with the value `"off"`.

Code:
- `crates/horch-core/src/launch.rs`: `overlay_skill_switches()` adds both keys.
  The plain overlay in `claude_command` calls it.
- `crates/horch-core/src/skills.rs`: `Bundle::claude_settings` calls the same
  function. This is the path that fleet panes use. It merges the keys into the
  teammate's own `settings:` JSON, if the teammate has one.
- `crates/horch-core/src/teammates.rs`: adds the fields
  `inherit_claudeai_skills` and `disabled_skills`. It flags both as
  unsupported on non-Claude agents. The roster check rejects a blank
  `disabled_skills` entry, an entry that is also in `skills`, and
  `disabled_skills` together with `disable_skills`.
- `teammates/_template.md`: documents both fields.

## Live check: `skillOverrides` merges per key

This overlay was used for the check:

    {"disableBundledSkills":true,"disableWorkflows":true,"syncClaudeAiSkills":false,
     "skillOverrides":{"explain-diff-html":"off"}}

With this overlay, the skill list drops from 24 to 23. The removed skill is
`explain-diff-html`, and no skill is added. The operator's own
`explain-diff-notion: off` still applies. So a `skillOverrides` object in
`--settings` merges per key with the operator's user settings. It does not
replace them.

## Live check: step 5

This check ran from the repo root with `env -u ANTHROPIC_API_KEY` and
`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`. It used the overlay that the
orchestrator pane now gets:

    {"disableBundledSkills":true,"disableWorkflows":true,"enabledPlugins":{
     "clangd-lsp@claude-plugins-official":false,"code@matts-robot-skills":false,
     "ddd@matts-robot-skills":false,"herdr@matts-robot-skills":false},
     "syncClaudeAiSkills":false}

Result: exit code 0 and the reply `OK`, with no settings error. The init
message lists 8 skills and 0 `anthropic-skills` entries. `explain-diff-notion`
is absent. After the check, `~/.claude/skills/.trash` does not exist, and the
synced bucket still holds 11 skill directories and `manifest.json`.
