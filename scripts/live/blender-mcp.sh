#!/usr/bin/env bash
# Live check, ahujasid Blender MCP server (mcp-for-blender) of blender-builder,
# blender-qa and blender-3d-printer. Checks that the 3 teammate files carry
# the same server line, that the pinned flags resolve to PIN_SET, that the
# server starts as the teammates launch it, that tools/list has every tool
# that the teammates deny (so a rename upstream fails here), and that
# telemetry is off.
# Result file: docs/live-checks/blender-mcp.md. Needs uv/uvx, git and network
# for the first uvx download. Blender is not needed: tools/list and
# get_addon_status answer without it. Starts no model session.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY 2>/dev/null || true

ROOT=$PWD
SCRATCH="$ROOT/.worktrees/_scratch/live-blender-mcp"
mkdir -p "$SCRATCH"
FAILED=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; FAILED=1; }
skip() { echo "SKIP $1: $2"; }

TEAMMATES="blender-builder blender-qa blender-3d-printer"
MCP_REPO=https://github.com/ahujasid/mcp-for-blender
MCP_COMMIT=992233bb258975a05eec8b5691465c50c2831e34
# The full set that uv resolved for the pin on 2026-11-04 (Python 3.12).
PIN_SET="annotated-types==0.8.0 anyio==4.16.0 attrs==26.1.0 certifi==2026.10.23 cffi==2.1.1 click==8.5.0 cryptography==50.0.2 h11==0.16.0 httpcore==1.0.9 httpx==0.28.1 httpx-sse==0.4.3 idna==3.20 jsonschema==4.26.0 jsonschema-specifications==2025.9.1 mcp==1.30.0 pycparser==3.12 pydantic==2.14.2 pydantic-core==2.50.2 pydantic-settings==2.16.2 pyjwt==2.15.1 python-dotenv==1.2.4 python-multipart==0.0.33 referencing==0.37.0 rpds-py==2026.9.1 sse-starlette==3.6.0 starlette==1.10.0 typing-extensions==4.16.0 typing-inspection==0.4.4 uvicorn==0.54.0"

# Prints the JSON of the `blender:` line of a teammate file.
spec_of() { python3 -I -c 'import re,sys; l=next(l for l in open(sys.argv[1]) if re.match(r"\s+blender:\s*\{", l)); print(l.split(":",1)[1].strip())' "$1"; }
# Prints the mcp__blender__* entries of the disallowed_tools line, 1 per line.
denied_of() { sed -n 's/^disallowed_tools: \[\(.*\)\]$/\1/p' "$1" | tr ',' '\n' | tr -d ' ' | sed -n 's/^mcp__blender__//p'; }

# 1. The 3 teammates carry 1 server line, with telemetry off and the denied tools.
FIRST=teammates/blender-builder.md
SPEC=$(spec_of "$FIRST")
DENIED=$(denied_of "$FIRST")
same=1
for t in $TEAMMATES; do
  [ "$(spec_of "teammates/$t.md")" = "$SPEC" ] || { fail "teammate-lines" "teammates/$t.md has another blender: line"; same=0; }
  [ "$(denied_of "teammates/$t.md")" = "$DENIED" ] || { fail "teammate-lines" "teammates/$t.md denies other tools"; same=0; }
done
if ! grep -q '"env":{"DISABLE_TELEMETRY":"true"}' <<<"$SPEC"; then
  fail "teammate-lines" "the server line lacks env DISABLE_TELEMETRY=true"
elif ! grep -q "@$MCP_COMMIT\"" <<<"$SPEC"; then
  fail "teammate-lines" "the server line is not pinned to $MCP_COMMIT"
elif [ -z "$DENIED" ]; then
  fail "teammate-lines" "disallowed_tools has no mcp__blender__ tool"
elif [ $same = 1 ]; then
  pass "teammate-lines: 3 files, 1 server line, telemetry off, denied: $(tr '\n' ' ' <<<"$DENIED" | sed 's/ $//')"
fi

# 2. The pinned flags resolve to PIN_SET, and the source at the pin reads
#    DISABLE_TELEMETRY before it sends anything.
if command -v uv >/dev/null && command -v git >/dev/null; then
  [ -d "$SCRATCH/src/.git" ] || git clone -q "$MCP_REPO" "$SCRATCH/src"
  git -C "$SCRATCH/src" checkout -q "$MCP_COMMIT"
  ARGS=$(python3 -I -c 'import json,sys; a=json.loads(sys.argv[1])["args"]; print(" ".join(a[:a.index("--from")]))' "$SPEC")
  PY_VER=$(sed -n 's/.*--python \([^ ]*\).*/\1/p' <<<"$ARGS")
  EXCL=$(sed -n 's/.*--exclude-newer \([^ ]*\).*/\1/p' <<<"$ARGS")
  tr ' ' '\n' <<<"$ARGS" | sed -n '/^--with$/{n;p;}' >"$SCRATCH/with.txt"
  if [ -z "$PY_VER" ] || [ -z "$EXCL" ]; then
    fail "mcp-pin" "teammate args lack --python or --exclude-newer: $ARGS"
  else
    got=$(uv pip compile "$SCRATCH/src/pyproject.toml" "$SCRATCH/with.txt" --python-version "$PY_VER" \
          --exclude-newer "$EXCL" --no-header --no-annotate -q 2>&1 | tr '\n' ' ' | sed 's/ $//')
    if [ "$got" = "$PIN_SET" ]; then pass "mcp-pin: $(wc -w <<<"$got" | tr -d ' ') packages, mcp==1.30.0, httpx==0.28.1, exclude-newer $EXCL"
    else fail "mcp-pin" "resolved set differs: $got"; fi
  fi
  T="$SCRATCH/src/src/blender_mcp/telemetry.py"
  # Every HTTP post of telemetry and trajectory data checks config.enabled,
  # and DISABLE_TELEMETRY sets it to False.
  if grep -q '"DISABLE_TELEMETRY"' "$T" && grep -q 'self.config.enabled = False' "$T" \
     && [ "$(grep -c 'if not self.config.enabled' "$T")" -ge 2 ] \
     && grep -q 'if not telemetry.config.enabled' "$SCRATCH/src/src/blender_mcp/trajectory.py"; then
    pass "telemetry-source: DISABLE_TELEMETRY turns off events, screenshot upload and trajectory rows"
  else
    fail "telemetry-source" "the telemetry gate in $T changed; read it again"
  fi
else
  skip "mcp-pin" "uv or git not found"
  skip "telemetry-source" "uv or git not found"
fi

# 3. The server, launched as the teammates launch it.
cat >"$SCRATCH/mcp.py" <<'PY'
# MCP stdio client: launch the server from the teammate's spec, list its
# tools, then call get_addon_status (read-only; answers without Blender).
import json, os, subprocess, sys
spec = json.loads(sys.argv[1])
env = dict(os.environ)
env.pop("ANTHROPIC_API_KEY", None)
env.update(spec.get("env", {}))
p = subprocess.Popen([spec["command"], *spec["args"]], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=sys.stderr, env=env, text=True)
def send(m):
    p.stdin.write(json.dumps(m) + "\n"); p.stdin.flush()
def recv(i):
    while True:
        l = p.stdout.readline()
        if not l:
            sys.exit("server closed stdout")
        try:
            m = json.loads(l)
        except ValueError:
            continue
        if m.get("id") == i:
            return m
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {},
    "clientInfo": {"name": "live-blender-mcp", "version": "1"}}})
info = recv(1)["result"].get("serverInfo", {})
print("MCP server %s %s" % (info.get("name"), info.get("version")))
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
tools = sorted(t["name"] for t in recv(2)["result"]["tools"])
print("MCP tools %d %s" % (len(tools), " ".join(tools)))
send({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
      "params": {"name": "get_addon_status", "arguments": {}}})
r = recv(3)
res = r.get("result") or {}
err = "error" in r or bool(res.get("isError"))
text = "".join(c.get("text", "") for c in res.get("content", [])) or json.dumps(r.get("error"))
print("MCP call get_addon_status isError=%s %s" % (err, text[:200].replace("\n", " ")))
p.stdin.close(); p.terminate()
PY
if [ -n "${LIVE_BLENDER_MCP_SKIP_SERVER:-}" ]; then
  skip "mcp-server" "LIVE_BLENDER_MCP_SKIP_SERVER is set"
  skip "mcp-denied-tools" "LIVE_BLENDER_MCP_SKIP_SERVER is set"
elif ! command -v uvx >/dev/null; then
  skip "mcp-server" "uvx not found"
  skip "mcp-denied-tools" "uvx not found"
elif out=$(python3 -I "$SCRATCH/mcp.py" "$SPEC" 2>"$SCRATCH/mcp_stderr.log"); then
  printf '  %s\n' "${out//$'\n'/$'\n'  }"
  tools=" $(grep '^MCP tools' <<<"$out" | cut -d' ' -f4-) "
  if grep -q '^MCP call get_addon_status isError=False' <<<"$out"; then
    pass "mcp-server: $(grep '^MCP server' <<<"$out" | cut -d' ' -f3-), $(grep '^MCP tools' <<<"$out" | cut -d' ' -f3) tools; get_addon_status answers"
  else
    fail "mcp-server" "get_addon_status failed: $(grep '^MCP call' <<<"$out")"
  fi
  missing=""
  for d in $DENIED; do grep -q " $d " <<<"$tools" || missing="$missing $d"; done
  if [ -z "$missing" ]; then pass "mcp-denied-tools: every denied tool exists in tools/list"
  else fail "mcp-denied-tools" "tools/list lacks:$missing (renamed upstream: update disallowed_tools)"; fi
  if grep -qi 'Telemetry disabled via environment variable' "$SCRATCH/mcp_stderr.log"; then
    pass "mcp-telemetry: the server logs telemetry disabled"
  else
    skip "mcp-telemetry" "no 'Telemetry disabled' log line; read $SCRATCH/mcp_stderr.log"
  fi
else
  fail "mcp-server" "server did not start: $(tail -3 "$SCRATCH/mcp_stderr.log" | tr '\n' ' ')"
  skip "mcp-denied-tools" "server did not start"
fi

# Clean up the server clone. Keep the stderr log for review.
rm -rf "$SCRATCH/src"
exit $FAILED
