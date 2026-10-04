# B1 blender: a Blender teammate with the Blender MCP server, for the UE team

Unit slug: `blender`. Branch: `ds/blender`.

## GOAL

A `blender-artist` teammate (name may change with reason) that drives Blender
through an MCP server and hands assets to the Unreal teammates cleanly: it
models, retopologizes, UV-unwraps, sets up collision and LODs, and exports
to the UE project's import folder with the conventions UE expects. Plus one
house skill, `blender-ue-pipeline`, that carries those conventions.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`,
  `ai_docs/reports/unreal-engine-wave.md`, `teammates/ue-technical-artist.md`
  and `teammates/ue-tools-engineer.md` (the teammates it works with), and
  `ai_docs/reports/domain-skills/swift-teammates.md` (how an MCP server was
  vetted and pinned: package, version, launch args, telemetry).
- Research (web), and write the findings in the report with sources:
  - Which Blender MCP server to use. Candidates include `ahujasid/blender-mcp`
    and any official Blender Foundation / Blender Lab MCP server. Compare:
    maintenance, transport (stdio vs socket to a Blender add-on),
    whether Blender must be running with an add-on enabled, headless support
    (`blender --background` with a script), tool list, and security: an
    "execute arbitrary Python" tool means arbitrary code on the host; asset
    download tools (Poly Haven, Sketchfab, Hyper3D/Rodin) reach the network
    and may need API keys. Choose one; pin a version; disable telemetry; name
    the tools the teammate may use.
  - Never put an API key in a teammate file. If a feature needs one, leave it
    off and document how the operator enables it (an env var by path or
    name, as `app-release-preparer` does).
- Pipeline rules the skill must carry (check each against Epic's FBX/glTF
  import docs and Blender's exporter docs; cite them): units and scale
  (Blender metres vs UE centimetres, unit scale and "Apply Scalings"),
  forward/up axes, applied transforms, `SM_`/`SK_`/`T_`/`M_` naming,
  `UCX_`/`UBX_`/`USP_` collision prefixes, LOD naming, smoothing and
  tangents, triangulation, skeleton root and armature naming for skeletal
  meshes, texture sizes and power-of-two, glTF vs FBX choice, and export
  into a folder the UE teammate imports (never write `.uasset` from Blender;
  import is the UE teammate's job through `ue-editor-scripting`).
- The teammate: `base: fleet-worker`, `agent: claude`, model and effort per
  `teammates/README.md` (a builder: opus/medium), `offer_when: ["*.blend",
  "*.uproject"]`, `mcp_servers` with the pinned Blender server, the house
  skill plus `ue-editor-scripting` as `available_skills`, `disallowed_tools:
  [Agent]`, no fallback (Codex has no MCP parity here; say so). First move:
  check Blender is installed (`blender --version`) and the MCP connection
  works; else `BLOCKED`. Git LFS: `.blend` and exported binaries are
  LFS-tracked; follow the LFS lock rules in `ai_docs/plans/finish/u1-ue-lfs.md`.
- Update the UE personas that receive assets (`ue-technical-artist`,
  `ue-tools-engineer`) with one line each: Blender work goes to the Blender
  teammate; they import its exports.
- Count-dependent tests change (SKIP lists, validation counts): see the P-UE
  report "Test changes".

## FILES

own: `teammates/blender-artist.md`, `skills/blender-ue-pipeline/` and its
provenance/README row, the 2 UE persona lines (coordinate: U1 also edits
`teammates/ue-*.md`; keep to 1 line each, the orchestrator merges),
`teammates/README.md` (a Blender row), the test lists,
`ai_docs/reports/finish/blender.md`.

## STEPS

0. Worktree. 1. Research and choose (report first draft). 2. Skill. 3.
Teammate. 4. `horch teammates --check`; show the roster offers it with a
`.blend` file and hides it without. 5. Gate. Report.
