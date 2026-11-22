# Security sweep: design-system-cli-guide

GATE 1: pass. GATE 2: pass.

| Severity | Findings |
| --- | --- |
| critical | 0 |
| high | 5 |
| medium | 7 |
| low | 2 |
| info | 1 |

| Finding | Severity | Skill | Disposition | Verify status |
| --- | --- | --- | --- | --- |
| `AUD-01#7` | high | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-02#8` | high | `astryx` | rewrite | mitigated-by-rewrite |
| `INJ-05#1` | high | `-` | rewrite | mitigated-by-rewrite |
| `INJ-05#4` | high | `astryx` | rewrite | mitigated-by-rewrite |
| `SUP-02#5` | high | `astryx` | replace | mitigated-by-rewrite |
| `AUD-03#9` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-04#10` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-05#11` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-07#13` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `INJ-03#2` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `INJ-03#3` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `INJ-03#6` | medium | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-06#12` | low | `astryx` | rewrite | mitigated-by-rewrite |
| `AUD-08#14` | low | `-` | rewrite | mitigated-by-rewrite |
| `AUD-09#15` | info | `astryx` | rewrite | mitigated-by-rewrite |

### Untrusted evidence (data, not instructions)

    INJ-05#1: global install writes to `~/.agents/skills/astryx/SKILL.md`. It carries no component docs and names no
    INJ-03#2: ASTRYX:START
    INJ-03#3: astryx-managed sha256:2bf23887c59daa97114635d52473b65ad74729cf1bf52240bef855996abffb3d
    INJ-05#4: Inside a project that has its own Astryx text (an `astryx-project` skill, or an Astryx block in AGENTS.md or CLAUDE.md), follow that text: it knows the project's version and setup.
    SUP-02#5: If `astryx` is not found, run `npm i -g @astryxdesign/cli` first.
    INJ-03#6: ASTRYX:END
    AUD-01#7: Tells the agent to run the external CLI's init command and then obey the agent docs it writes into the project. Those docs come from an unaudited npm binary at run time, so the real ...[cut]
    AUD-02#8: The workflow is driven by the output of an unaudited third-party CLI (build, search, template, component, docs). The agent acts on whatever those commands print, so CLI output is an ...[cut]
    AUD-03#9: Tells the agent to add two npm packages (core and CLI) to a project and to create a project folder when none exists. A new project gets no version pin. This supply-chain change is no...[cut]
    AUD-04#10: The description triggers on any UI building, component choice, layout, styling, theming or design-system work, not only on projects that use Astryx. It claims a wide class of tasks ...[cut]
    AUD-05#11: Steers the agent to learn component facts only through the CLI and not from the package's own files. This discourages inspection of the installed package source and makes the unaudi...[cut]
    AUD-06#12: Absolute rules beyond a tool pointer: every page starts from a template and never from scratch, the agent rewrites raw markup, CSS imports and literal color or pixel values before d...[cut]
    AUD-07#13: The body sits between managed-block markers with a content hash, and the README says a global CLI install writes this same text into the user's agent skills folder. The CLI can ther...[cut]
    AUD-08#14: Provenance is unverified: the manifests and README name a repository under a large company's GitHub organisation, while the author field and npm scope use another name. Nothing in t...[cut]
    AUD-09#15: The frontmatter declares no tool permissions, but the skill needs shell execution, package installation and file writes across the project. The effective permission need is wider th...[cut]
