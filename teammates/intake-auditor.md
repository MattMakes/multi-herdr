---
name: intake-auditor
brief_description: Hidden skill-intake auditor. Started only by skill-importer.
# skill-importer spawns it by name through `intake spawn`; the orchestrator
# never picks it (spec section 6.4, H-2).
hidden: true
base: fleet-worker
agent: claude
# Sanctioned #58: subscription auto-mode read-only pre-approval; see live-check results.
allowed_tools: ["Read", "Grep", "Glob", "Bash(git status)", "Bash(git status *)", "Bash(git log)", "Bash(git log --oneline -10)", "Bash(git diff)", "Bash(git diff --stat)", "Bash(git diff --cached)", "Bash(git show)", "Bash(git ls-files)", "Bash(horch ledger status)", "Bash(horch ledger status *)", "Bash(horch ledger show *)", "Bash(horch ledger steps *)", "Bash(horch ledger events)", "Bash(horch ledger events *)", "Bash(aws sts get-caller-identity)", "Bash(aws s3 ls)", "Bash(gcloud projects list)", "Bash(az account show)", "Bash(kubectl get pods)", "Bash(docker ps)"]
phase: null
model: opus
system_prompt: sr_eng_system_prompt
# No fallback: a fallback launches with another harness and other rules, so
# the trust wall below would be gone. skill-importer spawns with --exact.
fallbacks: []
# high: it judges hostile input.
effort: high
compact_window: 200000
compact_at: 300000
permission_mode: auto
inherit_plugins: false
mcp_servers: {}
skills: [skill-intake]
# Side: contaminated: reads the quarantined source and the scan, writes audit findings.
tools: [Read, Grep, Glob, Write, Bash]
# The 4 common denies, then this role's path rules (spec section 6.4).
# Agent: fleet rule. Web tools run outside the sandbox, so they are denied.
disallowed_tools:
  - Agent
  - WebFetch
  - WebSearch
  - NotebookEdit
  - "Edit(~/.local/state/skill-intake/drafts/**)"
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
  - "Bash(git *)"
# No network at all: no network.allowedDomains.
# report: horch opens no state root, ledger folder or herdr socket to this
# role, only 1 report socket for its pane (progress, tell, done).
# See docs/security/sandboxed-pane-horch-access.md.
horch_access: report
# `B` = ~/.local/state/skill-intake, fixed in fleet mode (spec section 9.3).
sandbox:
  filesystem:
    allowRead:
      - "~/.local/state/skill-intake/contaminated"
    allowWrite:
      - "~/.local/state/skill-intake/contaminated"
    denyRead:
      - "~/.local/state/skill-intake/drafts"
      - "~/.local/state/skill-intake/clean"
      - "~/.ssh"
      - "~/.aws"
      - "~/.config"
      - "~/Library/Keychains"
# Stale external copies of the fleet briefing; the repo carries the real one.
disabled_skills: [herdr-orchestrator, herdr-worker]
---
Follow `roles/auditor.md` of the `skill-intake` skill. Your task text names
your brief file. Read it and do only what it says.
