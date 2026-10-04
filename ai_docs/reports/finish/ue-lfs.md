# U1 ue-lfs: Unreal teammates on the latest engine and Git LFS

Branch `ds/ue-lfs`. Worker opus-57. Date 2026-10-04.

## Result

- The latest released Unreal Engine is **5.8**, hotfix **5.8.3** (2026-09-22). 5.8 is the version the 31 vendored `ue-*` skills target, so they stay verbatim and their advice is current. No 5.9 or UE6 release exists.
- Persona rule 3 now names the version: "The `ue-*` skills target UE 5.8, the latest release on 2026-10-04 (5.8.3)."
- Git LFS replaces every Perforce rule in the 11 `teammates/ue-*.md` personas and in the 2 house skills `ue-build-verify` and `ue-editor-scripting`.
- New file: `skills/ue-build-verify/references/git-lfs.md`. It has the setup (`git lfs install`, `git lfs pull` in a new worktree, the shared `.git/lfs`), the recommended `.gitattributes`, the lock rules, the read-only signal, and a pre-commit check.
- `grep -rni 'perforce\|p4 ' teammates/ue-* skills/ue-build-verify skills/ue-editor-scripting` prints nothing.

## Version research (sources)

| Fact | Source |
|---|---|
| UE 5.8 released 2026-06-17; "last planned major release" of UE5, 5.9 only "if needed" | https://www.unrealengine.com/news/unreal-engine-5-8-is-now-available, https://forums.unrealengine.com/t/unreal-engine-5-8-released/2729274 |
| 5.8.1 hotfix 2026-07-28 | https://forums.unrealengine.com/t/5-8-1-hotfix-released/2738864 |
| 5.8.2 hotfix 2026-08-25 | https://forums.unrealengine.com/t/5-8-2-hotfix-released/2746335 |
| 5.8.3 hotfix 2026-09-22 (latest) | https://forums.unrealengine.com/t/5-8-3-hotfix-released/2833315 |

## Re-check of W2's "not verified" flags (Epic 5.8 docs, 2026-10-04)

| Item | Before | After | Evidence |
|---|---|---|---|
| `Build.bat <Target> <Platform> <Config>` | no | **yes** | Epic 5.8 "Create an Installed Build": `Engine\Build\BatchFiles\Build.bat ShaderCompileWorker Win64 Development` |
| Linux editor binary | no | **yes** for `Engine/Binaries/Linux/UnrealEditor`; `-Cmd` name on macOS/Linux still no | Epic 5.8 "Linux Development Quickstart" |
| Launcher default folders `UE_5.8` | no | no | Epic 5.8 "Install Unreal Engine" gives no path |
| `Mac/Build.sh`, `Linux/Build.sh`, `-Project` on UBT, `Automation List`, `-nopause`, `index.json`, Live Coding text, conflicting-instance text, `Install.ini` location | no | no | No 5.8 page states them (Run Automation Tests 5.8 page rechecked) |
| `unreal.load_class` | no | **yes** | Unreal Python 5.8, module `unreal` |
| `add_event_override` position type | no | **yes, `IntPoint`**; recipe fixed from `Vector2D` to `IntPoint` | Unreal Python 5.8, `BlueprintEditorLibrary` |
| `AssetRenameData` fields | no | **yes**: `(asset, new_package_path, new_name)` | Unreal Python 5.8, `AssetRenameData` |
| `create_asset` with `factory=None` | no | no; the 5.8 page types `factory` as `Factory` | Unreal Python 5.8, `AssetTools` |
| `Actor.set_actor_label` | no | no; the 5.8 `Actor` page lists only `get_actor_label` | Unreal Python 5.8, `Actor` |

The marks are updated in `skills/ue-build-verify/references/commands.md` and `skills/ue-editor-scripting/references/{commands.md,python-recipes.md}`. Each evidence table now says when it was rechecked.

## Git LFS sources

- git-lfs manual pages (https://github.com/git-lfs/git-lfs/tree/main/docs/man): `git-lfs-lock` (locks against the LFS server; the file must exist), `git-lfs-unlock` (needs a clean status; `--force` removes another user's lock), `git-lfs-ls-files` (`*` full object, `-` pointer), `git-lfs-config` (`lfs.storage` default `.git/lfs`; `lfs.setlockablereadonly` default `true`, read-only when not locked by the current user).
- Community UE setups with `--lockable` `.uasset`/`.umap`: stevestreeting.com (2020), miltoncandelero.github.io/unreal-git. No Epic 5.8 page documents Git LFS locks.

## Changes

- `skills/ue-build-verify/SKILL.md`: version line; pointer to `git-lfs.md`; step 2 adds `git lfs pull` in a new worktree; step 3 replaces the Perforce check with the LFS lock check; the Rules entry and the checklist use LFS; References lists `git-lfs.md`.
- `skills/ue-build-verify/references/git-lfs.md`: new.
- `skills/ue-build-verify/references/log-reading.md`: the "Permission denied" row names the LFS read-only cause.
- `skills/ue-editor-scripting/SKILL.md`: version line; step 2 adds "Lock the assets" (including World Partition `__ExternalActors__` files); step 6 drops `p4 opened`/`p4 diff`; the Rules entry uses LFS; `DONE:` lists held locks; the checklist adds the lock check.
- 11 `teammates/ue-*.md`: rule 3 names UE 5.8 / 5.8.3. Rule 4 (10 builders and planners) keeps the one-build rule and Live Coding, without Perforce. New rule 6 (10 personas): the LFS lock rules. `ue-code-reviewer` gets a rule 6 finding: a lockable file changed without an assignment or a lock.
- `teammates/README.md` (UE section): six rules, a Git LFS paragraph, and a link to `git-lfs.md`.
- `skills/provenance.json`: the `adaptation` text of the 2 house skills records the recheck and the LFS rules.

## Decisions

- **Rule 6, not a longer rule 4.** Rule 4 stays about builds. The LFS rules are a separate rule so a persona reads one fact set per rule.
- **Workers keep their locks.** Fleet workers do not push, and `git lfs unlock` needs a clean status. A worker lists its locks in `DONE:` and unlocks only when the orchestrator says so.
- **The orchestrator's assignment, not the lock owner name, says who may change an asset.** Panes on one machine often share one git user, so "locked by me" can be another pane's lock.
- **`lfs.setlockablereadonly=false` gotcha.** Then lockable files stay writable, so a writable file is not proof of a lock. The skill says to check `git lfs locks`.
- **`.gitattributes` lives in `ue-build-verify`.** The plan allowed `ue-project-context` guidance or `ue-build-verify`; `ue-project-context` is vendored, so the house skill carries it. Workers do not edit `.gitattributes` unless assigned.
- **No `.mp4` line.** The plan's check grep `p4 ` matches `*.mp4 `. The table uses `*.mov` and says to add other types (for example MPEG-4 video) the same way.

## Out of scope (noticed, not fixed)

- `ai_docs/reports/unreal-engine-wave.md`, `ai_docs/reports/domain-skills/ue-own.md` and `ai_docs/plans/domain-skills/w2-ue-own.md` still mention Perforce. They are history; this unit does not own them.
- The vendored `skills/ue-project-context/SKILL.md` mentions Perforce in its scan (version control detection). This unit must not edit vendored skills.
- `ue-project-context` (vendored, adapted by W1) has no LFS field in its template. A future unit can add "version control: Git LFS" to the adaptation.
- The editor's own source control provider (Epic's Git plugin or the community Git LFS 2 plugin) is not covered. The fleet uses the `git lfs` CLI.
- No engine is installed, so nothing was run against a real 5.8 tree.
- Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` GREEN on 2026-10-04; 877 tests passed, 0 failed.
