//! Quota-aware balancing: which teammate a spawn actually runs as (design
//! section 13). Moved from `balance_policy.rs` (A5).
//!
//! A teammate names `fallbacks`. When its usage pool cannot serve a spawn, a
//! fallback lends its LAUNCH settings (agent, model, args, env, ...) while the
//! persona, base, phase and skills stay the original's (D6). [`decide`] is
//! pure: no I/O, and the time arrives inside the [`QuotaView`].

use serde::{Deserialize, Serialize};

use crate::ids::{IdError, TeammateName};
use crate::routing::eligible::eligible_fallbacks;
use crate::routing::policy::BalanceMode;
use crate::routing::quota::{self, Assessment, QuotaView, State};
use crate::teammates::{effort_problem, Roster, Teammate};

/// Flags a spawn passes to the gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateFlags {
    /// Never substitute.
    pub exact: bool,
    /// Never refuse.
    pub force: bool,
}

/// One pool line for a REFUSED message and `horch route`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoolLine {
    pub pool: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum Decision {
    Spawn {
        teammate: String,
        note: Option<String>,
    },
    Substitute {
        original: String,
        via: String,
        reason: String,
    },
    Refuse {
        teammate: String,
        reason: String,
        pools: Vec<PoolLine>,
    },
}

impl Decision {
    /// The line printed to stdout before the pane id, if any.
    pub fn line(&self) -> Option<String> {
        match self {
            Decision::Spawn { note, .. } => note.as_ref().map(|n| format!("NOTE: {n}")),
            Decision::Substitute {
                original,
                via,
                reason,
            } => Some(format!(
                "SUBSTITUTED: {original} runs on {via}. Reason: {reason}."
            )),
            Decision::Refuse {
                teammate, pools, ..
            } => {
                let pools: Vec<String> = pools
                    .iter()
                    .map(|p| format!("{} {} ({}).", p.pool, p.state, p.detail))
                    .collect();
                Some(format!(
                    "REFUSED: {teammate} cannot start. {} Options: wait, --force, or choose a teammate yourself.",
                    pools.join(" ")
                ))
            }
        }
    }
}

// ─── merge ──────────────────────────────────────────────────────────────────

/// `original` launched with `fallback`'s settings (section 13.2).
///
/// From the fallback: every field that says HOW to launch - agent, model,
/// args, env, tool lists, permission mode, plugins, MCP, settings, and the
/// rest of the Claude-shaped fields. From the original: name, description,
/// base, persona, phase, skills, generic, hidden. Effort stays the
/// original's when the fallback's agent and model accept it.
pub fn merge(original: &Teammate, fallback: &Teammate) -> Teammate {
    let mut m = fallback.clone();
    m.name = original.name.clone();
    m.brief_description = original.brief_description.clone();
    m.base = original.base.clone();
    m.persona = original.persona.clone();
    m.phase = original.phase;
    m.skills = original.skills.clone();
    m.generic = original.generic;
    m.hidden = original.hidden;
    m.first_instruction = original.first_instruction.clone();
    m.fallbacks = original.fallbacks.clone();
    m.effort = match &original.effort {
        Some(e) if effort_problem(fallback.agent, fallback.model.as_deref(), e).is_none() => {
            Some(e.clone())
        }
        _ => fallback.effort.clone(),
    };
    m
}

// ─── decide ─────────────────────────────────────────────────────────────────

/// `7d 88%, pace 1.4x, resets Fri 14:00Z`: an assessment in a few words.
pub fn summary(a: &Assessment, view: &QuotaView) -> String {
    let Some(w) = &a.worst else {
        return a.reason.clone();
    };
    let mut s = format!("{} {}", w.label(), quota::pct(w.used_at(view.now)));
    if let Some(pace) = w.pace(view.now) {
        if a.state == State::Tight && pace > view.policy.pace_factor {
            s.push_str(&format!(", pace {pace:.1}x"));
        }
    }
    if let Some(r) = &w.resets_at {
        s.push_str(&format!(", resets {}", quota::short_time(r)));
    }
    if a.state == State::Exhausted && !a.reason.contains('%') {
        return a.reason.clone();
    }
    s
}

/// The one-line reason a substitution records: `claude 7d 100%, resets
/// 2026-10-02T14:00Z`.
fn reason_line(a: &Assessment, view: &QuotaView) -> String {
    match &a.worst {
        Some(w) if a.state != State::Unknown && a.reason.contains('%') => {
            let mut s = format!(
                "{} {} {}",
                a.pool,
                w.label(),
                quota::pct(w.used_at(view.now))
            );
            if let Some(r) = w.resets_at.as_deref().and_then(crate::clock::parse) {
                let r = r + chrono::Duration::seconds(30);
                s.push_str(&format!(", resets {}", r.format("%Y-%m-%dT%H:%MZ")));
            }
            s
        }
        _ => format!("{} {}: {}", a.pool, a.state, a.reason),
    }
}

/// A fallback the gate may use, with its pool's state.
struct Candidate {
    name: String,
    state: State,
    assessment: Assessment,
}

/// The fallbacks the gate may use: the eligible ones, in `fallbacks:` order.
fn candidates(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<Candidate> {
    eligible_fallbacks(req, roster, view)
        .into_iter()
        .filter(|e| e.fallback_index.is_some() && e.is_eligible())
        .map(|e| {
            let model = e.model.as_ref().map(|m| m.as_str()).unwrap_or_default();
            let a = view.assess(e.harness.as_str(), model);
            Candidate {
                name: e.teammate.to_string(),
                state: a.state,
                assessment: a,
            }
        })
        .collect()
}

fn pool_line(a: &Assessment, view: &QuotaView) -> PoolLine {
    PoolLine {
        pool: a.pool.clone(),
        state: a.state.to_string(),
        detail: summary(a, view),
    }
}

/// What the spawn gate does with a request (section 13.4, BAL-03).
pub fn decide(
    req: &Teammate,
    roster: &Roster,
    view: &QuotaView,
    mode: BalanceMode,
    flags: GateFlags,
) -> Decision {
    let spawn = |note: Option<String>| Decision::Spawn {
        teammate: req.name.clone(),
        note,
    };
    if mode == BalanceMode::Off {
        return spawn(None);
    }
    let a = view.assess(req.agent.as_str(), req.model.as_deref().unwrap_or_default());
    if a.state == State::Ok {
        return spawn(None);
    }
    let cands = candidates(req, roster, view);
    let first_ok = cands.iter().find(|c| c.state == State::Ok);
    let pool_note = |extra: Option<&Candidate>| {
        let mut n = format!("{} pool {} ({}).", a.pool, a.state, summary(&a, view));
        if let Some(c) = extra {
            n.push_str(&format!(" Fallback {} is {}.", c.name, c.state));
        }
        n
    };
    let substitute = |c: &Candidate| Decision::Substitute {
        original: req.name.clone(),
        via: c.name.clone(),
        reason: reason_line(&a, view),
    };
    let refuse = || {
        let mut pools = vec![pool_line(&a, view)];
        for c in &cands {
            if !pools.iter().any(|p| p.pool == c.assessment.pool) {
                pools.push(pool_line(&c.assessment, view));
            }
        }
        Decision::Refuse {
            teammate: req.name.clone(),
            reason: reason_line(&a, view),
            pools,
        }
    };
    let refuse_or_force = || {
        if flags.force {
            spawn(Some(format!(
                "{} Spawned anyway (--force).",
                pool_note(None)
            )))
        } else {
            refuse()
        }
    };

    if mode == BalanceMode::Advise {
        let hint = first_ok.or_else(|| cands.iter().find(|c| c.state == State::Tight));
        return spawn(Some(pool_note(hint)));
    }

    // mode auto
    match a.state {
        State::Ok => spawn(None),
        State::Tight => {
            let Some(c) = first_ok else {
                return spawn(Some(pool_note(None)));
            };
            let theirs = c.assessment.headroom_per_h.unwrap_or(0.0);
            let ours = a.headroom_per_h.unwrap_or(0.0);
            if theirs >= view.policy.headroom_ratio * ours && !flags.exact {
                substitute(c)
            } else {
                spawn(Some(pool_note(Some(c))))
            }
        }
        State::Exhausted | State::Broken | State::Cooling => {
            let usable = cands
                .iter()
                .find(|c| matches!(c.state, State::Ok | State::Tight));
            match usable {
                Some(c) if !flags.exact => substitute(c),
                _ => refuse_or_force(),
            }
        }
        State::Unknown => match first_ok {
            Some(c) if !flags.exact => substitute(c),
            c => spawn(Some(pool_note(c))),
        },
    }
}

/// The teammate a decision launches: the request, or the request merged with
/// its fallback.
pub fn resolve(req: &Teammate, roster: &Roster, decision: &Decision) -> Option<Teammate> {
    match decision {
        Decision::Spawn { .. } => Some(req.clone()),
        Decision::Substitute { via, .. } => roster.get(via).map(|f| merge(req, f)),
        Decision::Refuse { .. } => None,
    }
}

// ─── provenance ─────────────────────────────────────────────────────────────

/// How routing chose the teammate a record runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// The gate in balance mode `auto`, with no flag.
    Auto,
    /// Balance mode `advise`: a note at most.
    Advise,
    /// Balance mode `off`.
    Off,
    /// `auto` with `--exact`: never substitute.
    Exact,
    /// `auto` with `--force`: never refuse.
    Force,
    /// A competition candidate on a fixed teammate (B3).
    Pinned,
    /// `horch spawn --resume`: the record's routing, kept.
    Resume,
    /// No gate ran: the `none` agent spends nothing.
    Ungated,
}

impl RoutingMode {
    /// The mode of one gate run.
    pub fn for_gate(mode: BalanceMode, flags: GateFlags) -> RoutingMode {
        match mode {
            BalanceMode::Off => RoutingMode::Off,
            BalanceMode::Advise => RoutingMode::Advise,
            BalanceMode::Auto if flags.exact => RoutingMode::Exact,
            BalanceMode::Auto if flags.force => RoutingMode::Force,
            BalanceMode::Auto => RoutingMode::Auto,
        }
    }
}

/// What routing decided for one record (ARC-14). Written next to `via` and
/// `substitution_reason`, which stay for older readers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingProvenance {
    /// The teammate the spawn asked for.
    pub requested: TeammateName,
    /// The teammate whose launch settings ran: the request, or its fallback.
    pub resolved: TeammateName,
    /// `Some(i)` when `resolved` is `fallbacks[i]` of the request.
    pub fallback_index: Option<u32>,
    /// The pool the resolved launch draws from, and its state at decision
    /// time.
    pub pool: String,
    pub pool_state: State,
    /// The gate's note or substitution reason, if any.
    pub reason: Option<String>,
    pub mode: RoutingMode,
}

fn fallback_index(req: &Teammate, name: &str) -> Option<u32> {
    req.fallbacks
        .iter()
        .position(|f| f == name)
        .map(|i| i as u32)
}

impl RoutingProvenance {
    /// The provenance of a gate decision; `None` for a refusal, which
    /// writes no record.
    pub fn from_decision(
        req: &Teammate,
        roster: &Roster,
        view: &QuotaView,
        decision: &Decision,
        mode: RoutingMode,
    ) -> Result<Option<RoutingProvenance>, IdError> {
        let (launch, reason) = match decision {
            Decision::Spawn { note, .. } => (req, note.clone()),
            Decision::Substitute { via, reason, .. } => match roster.get(via) {
                Some(f) => (f, Some(reason.clone())),
                None => return Ok(None),
            },
            Decision::Refuse { .. } => return Ok(None),
        };
        let a = view.assess(
            launch.agent.as_str(),
            launch.model.as_deref().unwrap_or_default(),
        );
        Ok(Some(RoutingProvenance {
            requested: TeammateName::new(req.name.as_str())?,
            resolved: TeammateName::new(launch.name.as_str())?,
            fallback_index: match decision {
                Decision::Substitute { via, .. } => fallback_index(req, via),
                _ => None,
            },
            pool: a.pool,
            pool_state: a.state,
            reason,
            mode,
        }))
    }

    /// A spawn no gate looked at: the teammate runs as itself.
    pub fn ungated(req: &Teammate) -> Result<RoutingProvenance, IdError> {
        let name = TeammateName::new(req.name.as_str())?;
        Ok(RoutingProvenance {
            requested: name.clone(),
            resolved: name,
            fallback_index: None,
            pool: quota::pool_for(req.agent.as_str(), req.model.as_deref().unwrap_or_default())
                .to_string(),
            pool_state: State::Unknown,
            reason: None,
            mode: RoutingMode::Ungated,
        })
    }

    /// The provenance of a record written before A5, from its `tier`,
    /// `via`, `agent`, `model` and `substitution_reason`. The pool state
    /// then is unknown. `req` is the requested teammate, if it still exists.
    pub fn legacy(
        tier: &str,
        via: Option<&str>,
        agent: &str,
        model: &str,
        reason: Option<&str>,
        req: Option<&Teammate>,
    ) -> Result<RoutingProvenance, IdError> {
        Ok(RoutingProvenance {
            requested: TeammateName::new(tier)?,
            resolved: TeammateName::new(via.unwrap_or(tier))?,
            fallback_index: via.zip(req).and_then(|(v, r)| fallback_index(r, v)),
            pool: quota::pool_for(agent, model).to_string(),
            pool_state: State::Unknown,
            reason: reason.map(str::to_owned),
            mode: RoutingMode::Off,
        })
    }

    /// The same routing, carried into a resume.
    pub fn resumed(self) -> RoutingProvenance {
        RoutingProvenance {
            mode: RoutingMode::Resume,
            ..self
        }
    }
}

/// A [`Decision`] in typed form. The JSON stays [`Decision`]'s.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingDecision {
    Spawn {
        teammate: String,
    },
    Substitute {
        requested: String,
        resolved: String,
        reason: String,
    },
    Refuse {
        requested: String,
        reason: String,
        pools: Vec<PoolLine>,
    },
}

impl From<&Decision> for RoutingDecision {
    fn from(d: &Decision) -> RoutingDecision {
        match d {
            Decision::Spawn { teammate, .. } => RoutingDecision::Spawn {
                teammate: teammate.clone(),
            },
            Decision::Substitute {
                original,
                via,
                reason,
            } => RoutingDecision::Substitute {
                requested: original.clone(),
                resolved: via.clone(),
                reason: reason.clone(),
            },
            Decision::Refuse {
                teammate,
                reason,
                pools,
            } => RoutingDecision::Refuse {
                requested: teammate.clone(),
                reason: reason.clone(),
                pools: pools.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::eligible::trains_on_input;
    use crate::routing::policy::Policy;
    use crate::routing::quota::QuotaFile;

    /// `candidates()` as it was before A5, for the comparison.
    fn old_candidates(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<Candidate> {
        req.fallbacks
            .iter()
            .filter_map(|name| roster.get(name))
            .filter(|f| !f.hidden && Roster::is_spawnable(f).is_ok() && !trains_on_input(f))
            .map(|f| {
                let a = view.assess(f.agent.as_str(), f.model.as_deref().unwrap_or_default());
                Candidate {
                    name: f.name.clone(),
                    state: a.state,
                    assessment: a,
                }
            })
            .collect()
    }

    fn summary_of(c: &[Candidate]) -> Vec<(String, State, Assessment)> {
        c.iter()
            .map(|c| (c.name.clone(), c.state, c.assessment.clone()))
            .collect()
    }

    #[test]
    fn arc_13_candidates_equivalent() {
        let mut roster = Roster::builtin().unwrap();
        let mut reserved = roster.require("codex-sol").unwrap().clone();
        reserved.name = "astra-worker".into();
        reserved.model = Some("gpt-6-astra".into());
        roster.insert_for_test(reserved);
        let names: Vec<String> = roster.names().into_iter().map(str::to_owned).collect();
        // Every drop rule in one list: missing, hidden, reserved, trains on
        // input, and usable ones around them.
        let mixed: Vec<String> = [
            "ghost",
            "codex-sol",
            "smoke",
            "astra-worker",
            "opencode-pickle",
            "sonnet",
            "codex-terra",
        ]
        .map(String::from)
        .to_vec();
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry/quota");
        let mut fixtures = 0;
        let mut compared = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let view = QuotaView::new(
                QuotaFile::read(&entry.path()).unwrap(),
                crate::clock::parse("2026-09-28T18:00:00Z").unwrap(),
                Policy::default(),
                true,
            );
            fixtures += 1;
            for name in &names {
                let t = roster.require(name).unwrap().clone();
                let mut m = t.clone();
                m.fallbacks = mixed.clone();
                for req in [t, m] {
                    assert_eq!(
                        summary_of(&candidates(&req, &roster, &view)),
                        summary_of(&old_candidates(&req, &roster, &view)),
                        "{name} on {}",
                        entry.path().display()
                    );
                    compared += 1;
                }
            }
        }
        assert_eq!(fixtures, 12);
        assert!(compared >= 12 * 33 * 2);
    }
}
