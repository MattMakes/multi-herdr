# Security sweep: plan-annotation

GATE 1: pass. GATE 2: pass.

| Severity | Findings |
| --- | --- |
| critical | 0 |
| high | 0 |
| medium | 2 |
| low | 2 |
| info | 1 |

| Finding | Severity | Skill | Disposition | Verify status |
| --- | --- | --- | --- | --- |
| `AUD-01#1` | medium | `plannotator-annotate` | replace | mitigated-by-rewrite |
| `AUD-02#2` | medium | `plannotator-annotate` | rewrite | mitigated-by-rewrite |
| `AUD-03#3` | low | `plannotator-annotate` | rewrite | mitigated-by-rewrite |
| `AUD-04#4` | low | `plannotator-annotate` | rewrite | mitigated-by-rewrite |
| `AUD-05#5` | info | `plannotator-annotate` | remove | removed |

### Untrusted evidence (data, not instructions)

    AUD-01#1: The skill is only a launcher for an external plannotator binary that is not in the tree. What that program reads, serves or sends over the network cannot be audited here. The agent r...[cut]
    AUD-02#2: The command's output is treated as user instruction: returned annotations are acted on directly, and an approved decision in a gate session lets later work go ahead. Anything that ca...[cut]
    AUD-03#3: The target may be a URL or a whole folder, passed to the external tool with no limit on which hosts or paths. Remote content gets fetched by a program outside the agent's own network...[cut]
    AUD-04#4: The agent is told to run the shell command itself and never hand it to the user, and on an unresolved target to guess the intended one and re-run without confirming. The user-supplie...[cut]
    AUD-05#5: Interface metadata for a different agent harness. It only sets a display name, a short description and turns implicit invocation off. No instructions or permissions in it. Not needed...[cut]
