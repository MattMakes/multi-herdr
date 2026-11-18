---
name: idea-lead
brief_description: Sub-orchestrator of 1 feature. Opus/medium, interactive. 1 process for each phase of 1 idea.
base: fleet-idea-lead
agent: claude
model: opus
# medium: the model guide default (design, "Worker profiles and adapters").
effort: medium
fallbacks: []
no_fallback_reason: "A fallback launches without the sandbox block of this file, so the worker sandbox of SR-8 would be gone. When the Opus pool is out, the task waits."
# Interactive: it interviews the operator in its herdr pane (design, "Exec or
# interactive": sub-orchestrators are interactive).
mode: interactive
system_prompt: sr_eng_system_prompt
compact_window: 200000
compact_at: 300000
permission_mode: auto
# Fleet rule: no subagents. FM-32: the profile denies the approve command and
# any write of approval.json; only fleetd writes that file.
disallowed_tools: [Agent, "Bash(horch idea approve*)",
                   "Edit(~/.horch/fleet/ideas/**/approval.json)"]
available_skills: [codebase-design]
# Stale external copies of the fleet briefing; the repo carries the real one.
disabled_skills: [herdr-orchestrator, herdr-worker,
                  "herdr:herdr-orchestrator", "herdr:herdr-worker"]
# The sandbox of a worker (SR-8). `~/.horch/fleet` is the fleet root beside
# the default ledger. It writes only its idea folders. It never reads the
# operator token, and it never writes into run/: the request folder and the
# channel socket are there. No entry of network.allowUnixSockets names run/.
sandbox:
  filesystem:
    allowWrite:
      - "~/.horch/fleet/ideas"
    denyRead:
      - "~/.horch/fleet/run/operator.token"
    denyWrite:
      - "~/.horch/fleet/run"
---
Your tier: OPUS - the sub-orchestrator of 1 feature. You run 1 phase of 1
idea, then your process ends. Your only memory is the idea folder.
