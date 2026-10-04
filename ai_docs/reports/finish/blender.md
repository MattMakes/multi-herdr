# B1 blender: a Blender teammate for the Unreal team

Branch `ds/blender`. Worker opus-58. Date 2026-10-04.
Plan: `ai_docs/plans/finish/b1-blender.md`.

## Result

| file | content |
|---|---|
| `teammates/blender-artist.md` | new teammate: claude, opus, medium, implementation; `offer_when: ["*.blend", "*.uproject"]`; Blender Lab MCP server pinned by commit |
| `skills/blender-ue-pipeline/SKILL.md`, `references/export-settings.md` | new house skill: 10,499 + 7,385 bytes (budget: 12 KB `SKILL.md`, 160 KB directory) |
| `skills/provenance.json`, `skills/README.md` | 1 entry (`sources: []`, own text), 1 row in the Unreal table |
| `teammates/ue-technical-artist.md`, `teammates/ue-tools-engineer.md` | 1 line each: Blender work goes to `blender-artist`; they import its exports |
| `teammates/README.md` | new section "The Blender teammate" (after the Unreal section, so U1's edits to that section do not overlap) |
| tests | `SKIP_NEW_TEAMMATES` + `blender-artist` (2 files); `REPO_ORIGINAL` + `blender-ue-pipeline`; Claude count 41 to 42 in `roster/validation.rs` |

- `horch teammates --check`: `roster ok: 60 teammates, 54 offered to the orchestrator`.
- Gate: green (see "Gate").

## Which Blender MCP server

Researched 2026-10-04.

| | Blender Lab `blender_mcp` (chosen) | `ahujasid/blender-mcp`, now `mcp-for-blender` |
|---|---|---|
| maker | Blender developers (Blender Lab) | community (Siddharth Ahuja) |
| source | [projects.blender.org/lab/blender_mcp](https://projects.blender.org/lab/blender_mcp), docs [blender.org/lab/mcp-server](https://www.blender.org/lab/mcp-server/) | [github.com/ahujasid/blender-mcp](https://github.com/ahujasid/blender-mcp) (MIT, about 30k stars, pushed 2026-09-30) |
| licence | GPL-3.0-or-later (`mcp/manifest.json`, SPDX headers, add-on manifest) | MIT |
| releases | v0.1.0 (2026-03-24) to v1.0.3 (2026-09-11, commit `2cea8d566dde07fbac28a61d698909d69724e853`) | PyPI `blender-mcp` 2.0.0 (2026-09-16) is a shim that installs `mcp-for-blender` 2.1.3 |
| package | not on PyPI; install from git, `mcp/` subdirectory, entry point `blender-mcp` | PyPI `mcp-for-blender` (and the old name `blender-mcp`) |
| transport | MCP stdio (or `--transport http`) to the server; TCP socket `localhost:9876` from the server to the add-on (`BLENDER_MCP_HOST`, `BLENDER_MCP_PORT`) | MCP stdio; TCP socket to the add-on (`BLENDER_HOST`, `BLENDER_PORT`, default 9876) |
| Blender running? | Live tools: yes, with the add-on (Blender 5.1+, `blender_version_min = "5.1.0"`). `*_for_cli` tools: no; they run `blender --background <file> --python-expr` (`BLENDER_PATH`, default `blender`) | yes, always, with the add-on |
| headless | yes: 6 `*_for_cli` tools; also `blender --background file.blend --command blender_mcp` serves the add-on headless | no |
| tools | 26: code execution (live and CLI), blend-file summaries (datablocks, missing files, linked libraries, path info, usage guess), object summaries, screenshots, renders, viewport jumps, API and manual search (bundled RST docs) | about 40: scene info, code execution, Poly Haven, Sketchfab, Poly Pizza, Hyper3D Rodin, Hunyuan3D, Tripo, viewport capture, export |
| network | none in its code: I cloned v1.0.3 (167 `.py` files) and found no `urllib`, `requests`, `httpx`, `http.client`, telemetry or analytics. Dependencies: `docutils`, `mcp[cli]>=1.2.0,<2`, `pyyaml` | asset search and download, AI generation services (API keys), and telemetry to Supabase including screenshot upload (`telemetry.py`); off with `DISABLE_TELEMETRY=true` |
| arbitrary code | `execute_blender_code`, `execute_blender_code_for_cli`; a "weak sandbox" blocks `sys.exit` and some operators, and says it is not a sandbox | `execute_blender_code`; opt-in AST allowlist `BLENDER_MCP_SAFE_MODE=1` |

**Choice: Blender Lab, v1.0.3, pinned by commit.** Reasons: it is made by
Blender developers; it has no telemetry and no network tools, so there is
nothing to turn off and no API key; and its `*_for_cli` tools work headless
with only a `blender` binary, which fits a fleet pane. The cost: it has fewer
tools, and the live tools need Blender 5.1+.

Launch line (in `teammates/blender-artist.md`):

```
uvx --from git+https://projects.blender.org/lab/blender_mcp.git@2cea8d566dde07fbac28a61d698909d69724e853#subdirectory=mcp blender-mcp
```

- Pinned by commit, not tag, because a tag can move. `git ls-remote` shows
  `v1.0.3` at that commit.
- **Gotcha: name collision.** `uvx blender-mcp` installs the ahujasid
  project from PyPI, not Blender Lab's. The persona comment says so.
- `mcp[cli]` floats inside `<2`; the pin covers the server code only.
- Telemetry: none to disable. `projects.blender.org` sits behind a
  Cloudflare check for browsers; `git ls-remote` and `git clone` over HTTPS
  worked from this host.
- `BLENDER_PATH` is not set in the teammate file, because it is a host
  path. The operator sets it in their environment when `blender` is not on
  PATH. Not verified: that Claude Code passes the pane's environment to a
  stdio MCP server that sets no `env`. If it does not, the operator puts
  `blender` on PATH.

### Tools the teammate may use

Named in the persona body: the `*_for_cli` tools (default), the 3 docs
tools, and the live tools only when the operator says a Blender with the
add-on is open for the pane. Not allowed: the 4 `jump_to_*` tools (they move
the operator's view). I did not deny them in `disallowed_tools`: the plan
sets `[Agent]`, and MCP tool-name patterns in that list are not tested in
this repository.

### Not done

- I did not run the MCP server (conventions §2: no package scripts).
- This host has Blender 3.5.1 at `/Applications/Blender.app`, not on PATH.
  The live tools need 5.1+. The headless path should work with 3.5.1
  (`--python-expr` exists), but I did not test it. The first real run of the
  teammate on this host will report `BLOCKED:` until `blender` is on PATH or
  `BLENDER_PATH` is set.

## The skill: `blender-ue-pipeline`

Workflow: check the tools; pick headless or live; read before you change;
a script in the repository; prepare; export one asset per file; verify in a
new process; write `<AssetName>.handoff.md`; report. Then 12 rules for Unreal,
Git LFS rules (from U1's plan), safety, and a checklist. The reference file
has the exporter calls with every value, a verification snippet, and an
evidence table with a source for each rule.

Decisions in the rules:
- **Scale:** Unit Scale 1.0, Apply Unit on, Apply Scalings `FBX All`. The
  API text defines the 4 options. That `FBX All` avoids a root bone scaled by
  100 comes from community reports, not Epic. The hand-off gives bounds in
  centimetres, so the UE teammate catches a wrong scale.
- **Axes:** exporter defaults (`-Z` forward, `Y` up). The mapping Blender
  `+X` to Unreal `+X`, `-Y` to `+Y` is not in Epic's or Blender's docs. The
  hand-off states the facing so the first import checks it.
- **Smoothing `FACE`**, not the API's preferred Normals Only, because
  Unreal warns about missing smoothing groups. **Triangulate** in Blender
  before baking (Epic: meshes must be triangulated).
- **LODs as separate files** by default. Not verified: that Blender's FBX
  exporter writes an FBX LOD group.
- **Armature object named `Armature`, root bone `root`.** Community
  sources, not Epic docs.
- **glTF** only for static props on request: whether Unreal's glTF importer
  reads `UCX_` children is not verified.
- **Normal maps:** Blender bakes OpenGL (green up); the hand-off asks the
  UE teammate to set Flip Green Channel.
- **Texture descriptors** `_BC`, `_N`, `_ORM` are a studio convention; Epic's
  naming page lists the prefixes only.

Versions: Blender 5.2 LTS is the latest release (5.2.2, 2026-09-15,
[release notes](https://developer.blender.org/docs/release_notes/5.2/)).
Unreal 5.8 is the latest
([5.8 release notes](https://dev.epicgames.com/documentation/unreal-engine/unreal-engine-5-8-release-notes)).

## Step 4: the orchestrator briefing

Method: the P-UE script (`ai_docs/reports/domain-skills/ue-teammates.md`,
step 3) as `/tmp/blender-demo.sh`, with the project file as an argument.
Built `horch fleet`, fake `herdr` (scenario `exec`), fake `claude`, `env -i`.

| project | briefing bytes | `blender-artist` line |
|---|---|---|
| `Asset.blend` | 15,656 | yes |
| empty | 15,520 | no |
| `Game.uproject` | 17,130 | yes (plus 11 `ue-` lines) |

The line costs 136 bytes, only on a Blender or Unreal project:

```
  blender-artist          Blender 3D for Unreal: models, retopology, UVs, collision, LODs, rigs; exports FBX for the UE team to import.
```

Gotcha for the next person: wait for the fake `claude` call that has
`--session-id`, not for any log line with it. The `herdr pane run` line also
contains it, and a `pkill` at that point kills the pane before it launches.

## Test changes

- `crates/horch-core/tests/baseline_oracles.rs`,
  `crates/horch-core/tests/skills_catalog.rs`: `blender-artist` in
  `SKIP_NEW_TEAMMATES` (sorted). No oracle or golden changed.
- `crates/horch-core/tests/skills_catalog.rs`: `blender-ue-pipeline` in
  `REPO_ORIGINAL` (a skill with `"sources": []`). The first gate run failed
  `skl_01_bundled_catalog_versions_and_digests` without it. The P-UE
  "Test changes" list does not name this list; a new own-text skill needs it.
- `crates/horch-core/src/roster/validation.rs`: Claude count 41 to 42. No
  phase arm change: implementation is the default arm.

## Gate

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: `GATE GREEN` (run 2, after the
`REPO_ORIGINAL` fix). Run 1 failed only `skl_01_bundled_catalog_versions_and_digests`.
No `GIT_DIR` or other git variable except `GIT_EDITOR` was set.

## Outside this unit's scope

- `offer.rs` has only an `xcode` requirement. A `blender` requirement for
  `horch doctor` would let the orchestrator see a missing Blender before a
  spawn. It is a code change outside this plan.
- Port 9876 is the default of both Blender MCP add-ons. An operator who has
  the ahujasid add-on running blocks the Blender Lab add-on.
