# Live check: ahujasid Blender MCP server

`blender-builder`, `blender-qa` and `blender-3d-printer` drive Blender
through the ahujasid server (`github.com/ahujasid/mcp-for-blender`, MIT),
hardened: pinned by commit, telemetry off, and the network asset tools
denied. This check proves the pin, the telemetry gate and the denied tool
names.

## How to run

```bash
scripts/live/blender-mcp.sh
```

It needs `uv`/`uvx`, `git`, `python3` and network for the first `uvx`
download. It does not need Blender: `tools/list` and `get_addon_status`
answer without it. It starts no model session, so it costs no tokens.

- `LIVE_BLENDER_MCP_SKIP_SERVER=1` skips the 2 server steps (no `uvx` launch).

The script writes only under `.worktrees/_scratch/live-blender-mcp/`. It
deletes the server clone at the end and keeps `mcp_stderr.log`.

Steps:

- `teammate-lines`: the 3 teammate files carry the same `blender:` line and
  the same denied tools. The line has `"env":{"DISABLE_TELEMETRY":"true"}`
  and the commit pin.
- `mcp-pin`: `uv pip compile` of the server's `pyproject.toml` with the
  teammate's `--with`, `--python` and `--exclude-newer` values gives the
  recorded set below.
- `telemetry-source`: at the pin, `telemetry.py` reads `DISABLE_TELEMETRY`
  and sets `config.enabled = False`. The event send, the screenshot upload
  and the trajectory rows each check `config.enabled` first.
- `mcp-server`: the server starts from the teammate line, answers
  `initialize`, `tools/list` and `get_addon_status`.
- `mcp-denied-tools`: each `mcp__blender__*` tool in `disallowed_tools`
  exists in `tools/list`. A rename upstream fails this step.
- `mcp-telemetry`: the server logs `Telemetry disabled via environment
  variable`. `SKIP` when the line is absent (see the note on `config.py`).

## The pin

- Commit `992233bb258975a05eec8b5691465c50c2831e34` (v2.2.6, 2026-10-29).
- Console script `mcp-for-blender` (`blender_mcp.server:main`).
- Python 3.12, `--exclude-newer 2026-11-02T00:00:00Z`.
- The 2 direct dependencies at exact versions: `mcp==1.30.0`,
  `httpx==0.28.1` (the source asks for `mcp>=1.26.0,<2`, `httpx>=0.27.0`).
- Env: `DISABLE_TELEMETRY=true`. The server also reads `BLENDER_HOST` and
  `BLENDER_PORT` (default `localhost:9876`).

The resolved set (29 packages, 2026-11-04, `uv pip compile`):

```
annotated-types==0.8.0 anyio==4.16.0 attrs==26.1.0 certifi==2026.10.23
cffi==2.1.1 click==8.5.0 cryptography==50.0.2 h11==0.16.0 httpcore==1.0.9
httpx==0.28.1 httpx-sse==0.4.3 idna==3.20 jsonschema==4.26.0
jsonschema-specifications==2025.9.1 mcp==1.30.0 pycparser==3.12
pydantic==2.14.2 pydantic-core==2.50.2 pydantic-settings==2.16.2
pyjwt==2.15.1 python-dotenv==1.2.4 python-multipart==0.0.33
referencing==0.37.0 rpds-py==2026.9.1 sse-starlette==3.6.0
starlette==1.10.0 typing-extensions==4.16.0 typing-inspection==0.4.4
uvicorn==0.54.0
```

## The tools

From the source at the pin: 14 `@mcp.tool` functions in
`src/blender_mcp/server.py`. Every tool is live: it needs Blender with the
`ahujasid` add-on on port 9876.

| tool | teammates |
|---|---|
| `get_addon_status`, `get_scene_info`, `execute_blender_code`, `look` | may use, live only, when the operator says the add-on is open |
| `search_mentions`, `open_viewport`, `viewport_latest`, `viewport_capture`, `viewport_pick` | not used (client UI tools) |
| `disable_telemetry`, `record_trajectory_feedback` | not used |
| `generate_3d`, `search_assets`, `import_asset` | denied in `disallowed_tools`: they generate models with paid providers or download from Poly Haven, Sketchfab and Poly Pizza |

## Telemetry

The server has 3 senders: `telemetry.py` (`_send_event` and
`upload_screenshot`) and `trajectory.py` (`_post_row`). Each checks
`config.enabled` first, and `DISABLE_TELEMETRY=true` sets it to `False`.
The PyPI version check in `update_cli.py` runs only for the `update`
subcommand, which the teammates do not run.

Note on `config.py`: `TelemetryCollector` imports `.config`, and
`src/blender_mcp/config.py` is in the repository's `.gitignore`. A
`git+https` install has no such file, so the collector cannot start, and
it has no Supabase URL to send to. Each caller catches that error. This is a
second reason why no telemetry leaves a teammate. It also means the
`Telemetry disabled` log line can be absent.

## 2026-11-04

| Step | Claim it proves | Tool version | Result | Evidence |
|---|---|---|---|---|
| teammate-lines | The 3 files carry 1 hardened server line. | - | PASS | `3 files, 1 server line, telemetry off, denied: generate_3d search_assets import_asset` |
| mcp-pin | The flags resolve to the set above. | uv | PASS | `29 packages, mcp==1.30.0, httpx==0.28.1, exclude-newer 2026-11-02T00:00:00Z` |
| telemetry-source | `DISABLE_TELEMETRY` stops every sender. | - | PASS | The script grep, and a read of `telemetry.py`, `trajectory.py`, `update_cli.py` and `server.py` at the pin. |
| mcp-server | The server starts as the teammates launch it. | - | not run (`LIVE_BLENDER_MCP_SKIP_SERVER=1`) | The worker's auto mode refused the `uvx` launch of the server. The tool list above comes from the source, not from `tools/list`: it is not verified. The operator runs the script. |
| mcp-denied-tools | The denied tool names exist. | - | not run | Same as `mcp-server`. |
