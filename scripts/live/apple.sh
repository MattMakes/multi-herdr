#!/usr/bin/env bash
# Live check, Apple family (U-21, U-54). Proves the MobileBuildMCP tool and
# workflow names that the Swift skills and teammates use, against the pinned
# server on this Mac, and checks the Xcode agent-skills export command.
# Result file: docs/live-checks/apple.md. Needs Xcode, node/npx and network
# for the first npx download. Starts no model session.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY 2>/dev/null || true

ROOT=$PWD
SCRATCH="$ROOT/.worktrees/_scratch/live-apple"
mkdir -p "$SCRATCH"
FAILED=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; FAILED=1; }
skip() { echo "SKIP $1: $2"; }

# MCP stdio client: launch the server exactly as the teammate's mcp_servers
# line says, with the workflow list replaced by $1 ("-" drops the variable,
# which gives the server default). Prints one tool name per line. With $2 and
# $3, it calls tool $2 with the JSON arguments $3 and prints the result JSON.
cat >"$SCRATCH/mcp.py" <<'PY'
import json, os, re, subprocess, sys
teammate, workflows = sys.argv[1], sys.argv[2]
line = next(l for l in open(teammate) if re.match(r"\s+mobilebuildmcp:\s*\{", l))
spec = json.loads(line.split(":", 1)[1])
env = dict(os.environ)
env.pop("ANTHROPIC_API_KEY", None)
env.update(spec.get("env", {}))
if workflows == "-":
    env.pop("MOBILEBUILDMCP_ENABLED_WORKFLOWS", None)
else:
    env["MOBILEBUILDMCP_ENABLED_WORKFLOWS"] = workflows
p = subprocess.Popen([spec["command"], *spec["args"]], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True)
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
    "clientInfo": {"name": "live-apple", "version": "1"}}})
info = recv(1)["result"].get("serverInfo", {})
print("server %s %s" % (info.get("name"), info.get("version")), file=sys.stderr)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
if len(sys.argv) > 3:
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
          "params": {"name": sys.argv[3], "arguments": json.loads(sys.argv[4])}})
    print(json.dumps(recv(2).get("result")))
else:
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    for t in recv(2)["result"]["tools"]:
        print(t["name"])
p.stdin.close(); p.terminate()
PY
mcp() { python3 -I "$SCRATCH/mcp.py" "$ROOT/teammates/swift-developer.md" "$@"; }

# 1. Xcode version.
XCODE_MAJOR=0
if command -v xcodebuild >/dev/null && XV=$(xcodebuild -version 2>/dev/null); then
  XCODE_MAJOR=$(sed -n 's/^Xcode \([0-9]*\).*/\1/p' <<<"$XV")
  pass "xcode-version: $(tr '\n' ' ' <<<"$XV")"
else
  skip "xcode-version" "xcodebuild not found"
fi

# 2. U-21: launch and list tools.
if ! command -v npx >/dev/null; then
  skip "mcp-tools" "npx not found"
  skip "mcp-skill-names" "npx not found"
  skip "mcp-workflows" "npx not found"
  skip "mcp-build" "npx not found"
else
  if mcp simulator,ui-automation,swift-package >"$SCRATCH/all.txt" 2>"$SCRATCH/server.txt"; then
    pass "mcp-tools: $(cat "$SCRATCH/server.txt"), $(wc -l <"$SCRATCH/all.txt" | tr -d ' ') tools with simulator,ui-automation,swift-package"
  else
    fail "mcp-tools" "handshake failed: $(cat "$SCRATCH/server.txt")"
  fi

  # Every tool name the Swift skills and teammates cite in backticks. The
  # snake_case names are tool names; the 4 one-word tools are listed here.
  NAMES=$( { git grep -h -o -E '`[a-z]+(_[a-z]+)+`' -- skills/ios-simulator-run skills/swift-code-audit \
               teammates/swift-*.md teammates/apple-*.md | tr -d '`'
             printf '%s\n' tap batch gesture screenshot; } | sort -u)
  MISSING=$(comm -23 <(echo "$NAMES") <(sort -u "$SCRATCH/all.txt") | tr '\n' ' ')
  if [ -z "$MISSING" ]; then
    pass "mcp-skill-names: $(echo "$NAMES" | wc -l | tr -d ' ') cited names all exist"
  else
    fail "mcp-skill-names" "not in tools/list: $MISSING"
  fi

  # Workflow claims: the default is simulator only; simulator carries
  # snapshot_ui and screenshot; tap needs ui-automation; swift_package_build
  # needs swift-package.
  mcp - >"$SCRATCH/default.txt" 2>/dev/null
  mcp simulator >"$SCRATCH/sim.txt" 2>/dev/null
  mcp ui-automation >"$SCRATCH/ui.txt" 2>/dev/null
  mcp swift-package >"$SCRATCH/spm.txt" 2>/dev/null
  has() { grep -qx "$1" "$SCRATCH/$2"; }
  if cmp -s "$SCRATCH/default.txt" "$SCRATCH/sim.txt" \
     && has snapshot_ui sim.txt && has screenshot sim.txt && ! has tap sim.txt \
     && has tap ui.txt && has type_text ui.txt && has gesture ui.txt && has batch ui.txt \
     && has swift_package_build spm.txt && ! has swift_package_build sim.txt; then
    pass "mcp-workflows: default == simulator ($(wc -l <"$SCRATCH/sim.txt" | tr -d ' ') tools, has snapshot_ui+screenshot, no tap); ui-automation adds tap; swift-package adds swift_package_build"
  else
    fail "mcp-workflows" "workflow sets differ from the skill claims; see $SCRATCH/*.txt"
  fi

  # 3. Build a tiny Swift package through swift_package_build (no signing).
  if ! command -v swift >/dev/null; then
    skip "mcp-build" "swift not found"
  else
    PKG="$SCRATCH/Tiny"
    rm -rf "$PKG"; mkdir -p "$PKG/Sources/Tiny"
    cat >"$PKG/Package.swift" <<'SWIFT'
// swift-tools-version:5.9
import PackageDescription
let package = Package(name: "Tiny", targets: [.target(name: "Tiny")])
SWIFT
    echo 'public func tiny() -> Int { 42 }' >"$PKG/Sources/Tiny/Tiny.swift"
    OUT=$(mcp swift-package swift_package_build "{\"packagePath\":\"$PKG\"}" 2>/dev/null || true)
    echo "$OUT" >"$SCRATCH/build.json"
    if [ -n "$OUT" ] && grep -q '"status": "SUCCEEDED"' <<<"$OUT"; then
      pass "mcp-build: swift_package_build built $PKG"
    else
      fail "mcp-build" "swift_package_build did not succeed; see $SCRATCH/build.json"
    fi
    rm -rf "$PKG/.build"
  fi
fi

# 4. U-54: the Xcode 27 agent-skills export. horch doctor runs the same check.
if [ "$XCODE_MAJOR" -ge 27 ]; then
  if xcrun agent skills export --help >"$SCRATCH/export-help.txt" 2>&1 \
     && grep -q -- '--output-dir' "$SCRATCH/export-help.txt"; then
    pass "xcode-skills-export: xcrun agent skills export --help names --output-dir"
  else
    fail "xcode-skills-export" "xcrun agent skills export --help failed or has no --output-dir"
  fi
else
  skip "xcode-skills-export" "needs Xcode 27 or later; this host has Xcode $XCODE_MAJOR"
fi

exit "$FAILED"
