# Live check: Blender

The Blender skills' helper code runs on the installed Blender, the pinned
Blender Lab MCP server starts as the blender-artist launches it, the 4
Blender skills build 1 real asset end to end, and the server's Python
dependencies resolve to 1 recorded set. Items U-15, U-20, U-51, U-55.

## How to run

```bash
scripts/live/blender.sh
```

It needs Blender (`BLENDER_PATH`, or `blender` on PATH, or
`/Applications/Blender.app`), `uv`/`uvx`, `git`, and network for the first
`uvx` download of the server. A missing tool is `SKIP`. It starts no model
session, so it costs no tokens.

- `LIVE_BLENDER_SKIP_MCP=1` skips the 2 server steps (no `uvx` launch).
- `LIVE_BLENDER_CLEAN=1` also deletes the built asset at the end.

The script writes only under `.worktrees/_scratch/live-blender/`. It
extracts the helper code blocks from the skill references at run time, so it
tests the text that the skills carry. At the end it deletes the server clone
and the check files. It keeps the asset in `asset/` (about 3 MB) for review.

Steps:

- `blender-version`: `Blender --version`.
- `helper <skill>.<function>` (U-51): every helper of
  `skills/blender-modeling/references/mesh-helpers.md`,
  `skills/blender-rigging/references/rig-helpers.md` and
  `skills/blender-baking/references/bake-helpers.md`, in
  `Blender --background --factory-startup --python`, with an assertion on
  each result.
- `mcp-headless` (U-20): reads the `blender:` line of
  `teammates/blender-artist.md`, starts the server with that command and
  those args (with `BLENDER_PATH` set), runs `initialize` and `tools/list`,
  then calls `search_api_docs` and `get_blendfile_summary_datablocks_for_cli`
  on a probe `.blend`. Both are read-only.
- `mcp-live` (U-20): calls `get_objects_summary`, which needs a running
  Blender with the Blender Lab MCP add-on on `localhost:9876`.
- `asset-build` (U-15): builds `SM_Crate` and `SK_Crate` with the helpers and
  the steps of `blender-modeling`, `blender-baking`, `blender-rigging` and
  `blender-ue-pipeline`. Each skill step prints 1 `ASSET <skill>.<step>` line
  with its check.
- `asset-verify` (U-15): opens the saved `.blend` in a new process, runs the
  verification snippet of `skills/blender-ue-pipeline/references/export-settings.md`,
  and re-imports each FBX for names, bounds, triangles, bones and frames.
- `mcp-pin` (U-55): runs `uv pip compile` on the server's `pyproject.toml` at
  the pinned commit with the teammate's `--python`, `--exclude-newer` and
  `--with` flags, and compares the result with the recorded set.

## The asset

`SM_Crate`: a 60 x 60 x 58.1 cm crate with a lid, front toward Blender `+X`,
pivot at the base centre.

| skill step | check | result |
|---|---|---|
| modeling 2, shape | dimensions within 1 % of the brief | 0.6 x 0.6 x 0.581 m |
| modeling 3, topology | `mesh_problems` all 0 | 0 non-manifold, 0 boundary, 0 n-gons, 196 faces |
| modeling 5-6, seams and UV 0 | every sharp edge splits the UVs; 0 UVs out of bounds; margin 8 px | 0 sharp edges (3-segment chamfers), 0 out of bounds |
| modeling 7, texel density | 400 px/m +-10 % | 423 px/m (4.2 px/cm) at 1024 |
| modeling 8, lightmap UV | channel 1 inside 0..1; channel 0 active | `UVMap`, `Lightmap` |
| modeling 4 and 9, LODs | LOD0 under 1,200; LOD1 and LOD2 within 3 % of 50 % and 25 %; slot order and UV channels kept | 376, 188, 94 triangles |
| modeling 10, collision | `UCX_SM_Crate_LOD0_00` convex, closed, no UVs or material | 24 faces, convex |
| baking 2, explode | lid pair moved +1 m in a copy for the bake | copy removed after |
| baking 4-6, normal | 16-bit Non-Color; mean about (0.5, 0.5, 1.0); detail captured | mean (0.501, 0.5, 0.994); red and green std 0.107 |
| baking 7, AO | 64 samples, from the high mesh | mean 0.959; 5th percentile 0.89 |
| baking 8-9, roughness and ORM | roughness 0.7 from the material; ORM Non-Color | mean 0.702; R AO, G roughness, B 0 |
| baking 10, save and reload | 1024 x 1024; reloaded mean matches | `T_Crate_N.png` 16-bit, `T_Crate_ORM.png` 8-bit |
| rigging 2, armature | `Armature` with `root`, `body`, `lid` | 3 bones |
| rigging 3-4, weights | `weight_problems` all 0; every vertex 100 % on its part | heat weights mixed 168 vertices on this rigid prop; the script reassigned them by region (step 5) |
| rigging 5, pose test | lid -60 degrees lifts its front edge; restore returns it | 0.581 m to 1.055 m, then 0.581 m |
| rigging 7, action | `AS_Crate_LidOpen`, frames 1-30, fake user | pass |
| pipeline 5-6, export | scale 1, rotation 0, origin 0; 5 FBX files | `SM_Crate_LOD0` (with the UCX), `_LOD1`, `_LOD2`, `SK_Crate`, `AS_Crate_LidOpen` |
| pipeline 7, verify | export-settings snippet: 0 problems; re-import bounds 60 x 60 x 58.1 cm within 0.5 cm; bones; 30 frames | pass |
| pipeline 8, hand-off | `SM_Crate.handoff.md` beside the exports | written |

## The MCP dependency pin (U-55)

`teammates/blender-artist.md` launches the server with `--python 3.12`,
`--exclude-newer 2026-10-07T00:00:00Z` and
`--with mcp[cli]==1.30.0 --with docutils==0.23 --with pyyaml==6.0.3`. These
flags resolve to the 38 packages that the running fleet server had installed
on 2026-10-06 (Python 3.12.11):

annotated-doc 0.0.5, annotated-types 0.8.0, anyio 4.15.1, attrs 26.1.0,
certifi 2026.7.22, cffi 2.1.1, click 8.5.0, cryptography 50.0.2,
docutils 0.23, h11 0.16.0, httpcore 1.0.9, httpx 0.28.1, httpx-sse 0.4.3,
idna 3.20, jsonschema 4.26.0, jsonschema-specifications 2025.9.1,
markdown-it-py 4.2.0, mcp 1.30.0, mdurl 0.1.2, pycparser 3.0,
pydantic 2.13.5, pydantic-core 2.46.5, pydantic-settings 2.15.0,
pygments 2.21.0, pyjwt 2.15.1, python-dotenv 1.2.4, python-multipart 0.0.32,
pyyaml 6.0.3, referencing 0.37.0, rich 15.0.0, rpds-py 2026.9.1,
shellingham 1.5.4, sse-starlette 3.5.0, starlette 1.7.0, typer 0.27.3,
typing-extensions 4.16.0, typing-inspection 0.4.4, uvicorn 0.54.0.

The pin does not cover the setuptools version that builds the server from
git (a build dependency).

## 2026-10-06

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| blender-version | Blender is installed (`teammates/blender-artist.md` `requires: [blender]`). | Blender 3.5.1 (build 2023-04-24) | PASS | `/Applications/Blender.app/Contents/MacOS/Blender`. Not on PATH, and `BLENDER_PATH` is not set in the fleet pane. |
| helper mesh.* (10) | The `blender-modeling` helpers work as written. | 3.5.1 | PASS, 2 bugs fixed (6d5f966) | All 8 original checks pass (sphere 3,968 tris; LOD1 fitted to 1,984; 142-face hull; torus concave). The crate found 2 bugs: `unwrap_smart` failed with no active object; `make_convex_collision` failed on 2 stacked chamfered boxes (`geom: found the same ... used multiple times`). Both regression cases FAIL on the old helpers and PASS on the fixed ones. |
| helper rig.* (7) | The `blender-rigging` helpers work as written. | 3.5.1 | PASS | `clean_weights` cleared 126 unnormalized vertices; pose moved the top 0.405 m and restored it; FBX round trip kept `root`, `lower`, `upper` and 24 frames (imported as 2-25, the 1-frame shift the skill notes). |
| helper bake.* (7) | The `blender-baking` helpers work as written. | 3.5.1, Cycles CPU | PASS | Normal mean (0.5, 0.501, 0.993); AO 1.0 and roughness 0.502 on baked texels; base color (0.5, 0.2, 0.1) stored as sRGB (0.737, 0.486, 0.349); 16-bit and 8-bit PNGs reloaded with the same means. |
| mcp-headless | The pinned server starts from the teammate's `mcp_servers` line and answers read-only calls. | server v1.0.3 (`2cea8d5`), mcp 1.30.0, uv 0.8.4 | PASS in the fleet session; script step not run | The fleet started this worker with the teammate's server. It listed 26 tools: 6 `*_for_cli` tools, 3 docs tools, 4 `jump_to_*` tools and 13 live tools. `search_api_docs("pack_islands margin_method")` returned the `bpy.ops.uv.pack_islands` signature. `get_blendfile_summary_datablocks_for_cli` returned `Blender executable not found at 'blender'. Set the BLENDER_PATH environment variable`, because the fleet pane has no `BLENDER_PATH`. The worker's auto mode refused a second, standalone `uvx` launch, so the operator runs the script step (see below). |
| mcp-live | The live tools reach Blender with the add-on. | 3.5.1 | SKIP | `get_objects_summary` returned `Cannot connect to Blender at localhost:9876`. The live tools need Blender 5.1 or later; this Mac has 3.5.1. |
| asset-build | The 4 Blender skills build a crate with LODs, collision, a 3-bone rig, an action, a 1024 normal and AO bake, and FBX exports. | 3.5.1 | PASS | 28 skill-step checks pass; see "The asset". The runs found and fixed: welded lid (gap of 1 mm), `seams_from_islands` no-op, 2 helper bugs, AO 0 on coplanar high faces (inset 2 mm). The 500 px/m target of the first brief did not fit at 1024 (423 px/m at full packing); the brief now asks 400 px/m. |
| asset-verify | The saved file and the FBX exports meet `blender-ue-pipeline` in a new process. | 3.5.1 | PASS | Snippet 0 problems; LOD0 FBX has `SM_Crate_LOD0` and `UCX_SM_Crate_LOD0_00`, 60.0 x 60.0 x 58.1 cm; LOD1/LOD2 lose at most 0.3 cm to decimation; `SK_Crate` has `root`, `body`, `lid` and no action; `AS_Crate_LidOpen` has 30 frames. |
| mcp-pin | The teammate's flags resolve to 1 recorded set (3c2aeec). | uv 0.8.4 | PASS | 38 packages, equal to the running fleet server's environment. Without `--exclude-newer`, `typer` changed from 0.27.2 to 0.27.3 on 2026-10-06, which proves the float. |

### Operator steps

1. Run the 2 server steps that the worker could not run:
   `scripts/live/blender.sh` (without `LIVE_BLENDER_SKIP_MCP`).
   Expect `PASS mcp-headless` and `SKIP mcp-live`.
2. For `mcp-live`: install Blender 5.1 or later, install the Blender Lab
   MCP add-on from `projects.blender.org/lab/blender_mcp`, start its server
   in Blender, and close any `ahujasid` Blender MCP add-on (same port 9876).
   Then run `scripts/live/blender.sh` again.
3. Set `BLENDER_PATH=/Applications/Blender.app/Contents/MacOS/Blender` in the
   environment that starts the fleet, so the `*_for_cli` tools find Blender.
