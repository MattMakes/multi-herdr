---
name: intake-sanitizer
brief_description: Hidden skill-intake sanitizer. Started only by skill-importer.
# skill-importer spawns it by name through `intake spawn`; the orchestrator
# never picks it (spec section 6.4, H-2).
hidden: true
base: fleet-worker
agent: claude
# Sanctioned #58: subscription auto-mode read-only pre-approval; see live-check results.
allowed_tools: ["Read", "Grep", "Glob", "Bash(git status)", "Bash(git status *)", "Bash(git log)", "Bash(git log --oneline -10)", "Bash(git diff)", "Bash(git diff --stat)", "Bash(git diff --cached)", "Bash(git show)", "Bash(git ls-files)", "Bash(horch ledger status)", "Bash(horch ledger status *)", "Bash(horch ledger show *)", "Bash(horch ledger steps *)", "Bash(horch ledger events)", "Bash(horch ledger events *)", "Bash(aws sts get-caller-identity)", "Bash(aws s3 ls)", "Bash(gcloud projects list)", "Bash(az account show)", "Bash(kubectl get pods)", "Bash(docker ps)"]
phase: null
model: sonnet
# No fallback: a fallback launches with another harness and other rules, so
# the trust wall below would be gone. skill-importer spawns with --exact.
fallbacks: []
# sonnet, medium: it edits wording against a scan report.
effort: medium
compact_window: 200000
compact_at: 300000
permission_mode: auto
inherit_plugins: false
mcp_servers: {}
skills: [skill-intake]
# Side: source-blind: reads and edits the drafts only.
tools: [Read, Grep, Glob, Write, Edit, Bash]
# The 4 common denies, then this role's path rules (spec section 6.4).
# Agent: fleet rule. Web tools run outside the sandbox, so they are denied.
disallowed_tools:
  - Agent
  - WebFetch
  - WebSearch
  - NotebookEdit
  - "Read(~/.local/state/skill-intake/contaminated/**)"
  - "Edit(~/.local/state/skill-intake/contaminated/**)"
  - "Edit(~/.local/state/skill-intake/clean/**)"
  - "Bash(python*)"
  - "Bash(node*)"
  - "Bash(sh *)"
  - "Bash(bash *)"
  - "Bash(zsh *)"
  - "Bash(perl *)"
  - "Bash(ruby *)"
  - "Bash(./*)"
  - "Bash(source *)"
  - "Bash(chmod *)"
# No network at all: no network.allowedDomains. horch adds its own state
# root, ledger folder and sockets at launch; do not list them here.
# `B` = ~/.local/state/skill-intake, fixed in fleet mode (spec section 9.3).
sandbox:
  filesystem:
    allowRead:
      - "~/.local/state/skill-intake/drafts"
    allowWrite:
      - "~/.local/state/skill-intake/drafts"
    denyRead:
      - "~/.local/state/skill-intake/contaminated"
      - "~/.local/state/skill-intake/clean"
      - "~/.ssh"
      - "~/.aws"
      - "~/.config"
      - "~/Library/Keychains"
# Stale external copies of the fleet briefing; the repo carries the real one.
disabled_skills: [herdr-orchestrator, herdr-worker]
---
Follow `roles/sanitizer.md` of the `skill-intake` skill. Your task text names
your brief file. Read it and do only what it says.
