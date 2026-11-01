//! `multi-herdr-dataset status [<exp>]`: the experiments and rounds, folded
//! from the events, with their state.

use anyhow::{bail, Result};
use horch_core::competition::model::RoundState;
use horch_core::evaluation::validator::ValidationReport;
use horch_core::execution::FailureKind;
use horch_core::ids::{ExperimentId, RoundId};
use horch_core::measure::projection::{fold, CandidateView, ExperimentView, Projection, RoundView};
use horch_core::measure::store;
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};
use crate::output;

pub(crate) fn status(ctx: &RuntimeContext, experiment: Option<&str>) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let read = store::read_all(&paths)?;
    let projection = fold(&read.events);
    let only = experiment.map(ExperimentId::new).transpose()?;
    if let Some(exp) = &only {
        if !projection.experiments.contains_key(exp) {
            bail!("no experiment {exp} in {}", paths.root().display());
        }
    }
    output::print(&render(&projection, only.as_ref(), read.torn_lines));
    Ok(exit::SUCCESS)
}

/// The text `status` prints.
pub fn render(projection: &Projection, only: Option<&ExperimentId>, torn_lines: u32) -> String {
    let mut out = String::new();
    if projection.experiments.is_empty() {
        out.push_str("no experiments\n");
    }
    let shown = projection
        .experiments
        .iter()
        .filter(|(id, _)| only.is_none_or(|o| o == *id));
    for (id, exp) in shown {
        render_experiment(&mut out, id, exp, projection);
    }
    if !projection.anomalies.is_empty() {
        out.push_str(&format!(
            "{} anomalies (events not applied)\n",
            projection.anomalies.len()
        ));
    }
    if torn_lines > 0 {
        out.push_str(&format!("{torn_lines} torn lines skipped\n"));
    }
    out
}

/// The lines of one experiment: its header, its abort, then its rounds.
fn render_experiment(
    out: &mut String,
    id: &ExperimentId,
    exp: &ExperimentView,
    projection: &Projection,
) {
    out.push_str(&format!(
        "experiment {id} {} task {} candidates {} base {}\n",
        experiment_state(exp, projection).as_str(),
        exp.created.task_id,
        exp.created.candidates,
        short(&exp.created.base_sha),
    ));
    if let Some(aborted) = &exp.aborted {
        out.push_str(&format!(
            "  aborted: {} ({})\n",
            aborted.reason,
            aborted.failed_checks.join(", ")
        ));
    }
    let rounds = exp
        .rounds
        .iter()
        .filter_map(|id| Some((id, projection.rounds.get(id)?)));
    for (round_id, r) in rounds {
        render_round(out, round_id, r);
    }
}

/// The lines of one round: its header, then one line per candidate.
fn render_round(out: &mut String, round_id: &RoundId, r: &RoundView) {
    let winner = r
        .winner
        .as_ref()
        .map(|w| format!(" winner {}", w.label))
        .unwrap_or_default();
    out.push_str(&format!(
        "  round {round_id} #{} {} candidates {}{winner}\n",
        r.created.index,
        r.state.as_str(),
        r.candidates.len(),
    ));
    for (label, c) in &r.candidates {
        out.push_str(&format!("    {label} {}\n", candidate_line(c)));
    }
}

/// The state `status` shows for an experiment (dataset design §5). The
/// experiment's own state stops at PLANNED, so that it can take more rounds.
/// From its first round on, the experiment is where its latest round is.
pub fn experiment_state(exp: &ExperimentView, projection: &Projection) -> RoundState {
    match exp.state {
        RoundState::Planned => exp
            .rounds
            .last()
            .and_then(|r| projection.rounds.get(r))
            .map_or(exp.state, |r| r.state),
        state => state,
    }
}

/// One candidate: its teammate and how far it got.
fn candidate_line(c: &CandidateView) -> String {
    let who = c
        .planned
        .as_ref()
        .map(|p| p.teammate.to_string())
        .unwrap_or_else(|| "?".into());
    let ended = match (&c.failed, &c.completed, &c.validation) {
        (Some(f), _, Some(_)) => format!(" (failed {})", failure(&f.failure)),
        (_, Some(_), Some(_)) => " (completed)".to_string(),
        _ => String::new(),
    };
    format!("{who} {}{ended}", progress(c))
}

/// How far the candidate got, from the latest step it reached.
fn progress(c: &CandidateView) -> String {
    if let Some(v) = &c.validation {
        validated(v, c.is_eligible())
    } else if c.frozen.is_some() {
        "frozen".to_string()
    } else if let Some(f) = &c.failed {
        format!("failed {}", failure(&f.failure))
    } else if c.completed.is_some() {
        "completed".to_string()
    } else if c.spawned.is_some() {
        "running".to_string()
    } else if c.worktree.is_some() {
        "waiting".to_string()
    } else {
        "planned".to_string()
    }
}

/// The mechanical score of a validated candidate, and if it is eligible.
fn validated(v: &ValidationReport, eligible: bool) -> String {
    let suffix = if eligible { " eligible" } else { "" };
    format!("validated score {:.2}{suffix}", v.mechanical_score)
}

fn failure(f: &FailureKind) -> String {
    match f {
        FailureKind::AgentExited { code: Some(c) } => format!("agent_exited {c}"),
        FailureKind::AgentExited { code: None } => "agent_exited".into(),
        FailureKind::PaneVanished => "pane_vanished".into(),
        FailureKind::TimedOut => "timed_out".into(),
        FailureKind::Cancelled { reason } => format!("cancelled ({reason})"),
        FailureKind::Crashed => "crashed".into(),
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::test_fixtures::{candidate, exp, experiment, projection, round_view};

    fn line(parts: &[&str]) -> String {
        candidate_line(&candidate(parts))
    }

    #[test]
    fn candidate_line_names_each_of_the_seven_states() {
        assert_eq!(line(&[]), "? planned");
        assert_eq!(line(&["planned"]), "sonnet planned");
        assert_eq!(line(&["planned", "worktree"]), "sonnet waiting");
        assert_eq!(line(&["planned", "worktree", "spawned"]), "sonnet running");
        assert_eq!(
            line(&["planned", "worktree", "spawned", "completed"]),
            "sonnet completed"
        );
        assert_eq!(
            line(&["planned", "worktree", "spawned", "failed"]),
            "sonnet failed agent_exited 3"
        );
        assert_eq!(
            line(&["planned", "worktree", "spawned", "completed", "frozen"]),
            "sonnet frozen"
        );
        assert_eq!(
            line(&["planned", "frozen", "validation"]),
            "sonnet validated score 0.50 eligible"
        );
    }

    #[test]
    fn candidate_line_marks_an_ended_candidate_that_was_validated() {
        assert_eq!(
            line(&["planned", "completed", "frozen", "validation"]),
            "sonnet validated score 0.50 eligible (completed)"
        );
        assert_eq!(
            line(&["planned", "failed", "frozen", "validation"]),
            "sonnet validated score 0.50 eligible (failed agent_exited 3)"
        );
    }

    #[test]
    fn render_says_no_experiments_for_an_empty_projection() {
        assert_eq!(render(&Projection::default(), None, 0), "no experiments\n");
    }

    #[test]
    fn render_lists_the_round_and_its_candidates() {
        let mut p = projection(RoundState::Planned, &[RoundState::Running]);
        let round = p.rounds.values_mut().next().unwrap();
        *round = round_view(
            RoundState::Running,
            vec![
                ("A", candidate(&["planned", "worktree", "spawned"])),
                ("B", candidate(&[])),
            ],
        );
        let id = p.rounds.keys().next().unwrap().clone();
        assert_eq!(
            render(&p, None, 0),
            format!(
                "experiment {} RUNNING task task-1 candidates 2 base 0123456789ab\n  \
                 round {id} #0 RUNNING candidates 2\n    A sonnet running\n    B ? planned\n",
                exp()
            )
        );
    }

    #[test]
    fn render_with_only_skips_the_other_experiments() {
        let mut p = projection(RoundState::Planned, &[]);
        let other = ExperimentId::new("0199a5b0-0000-7000-8000-000000000e02").unwrap();
        p.experiments
            .insert(other.clone(), experiment(RoundState::Planned, Vec::new()));
        let text = render(&p, Some(&other), 0);
        assert!(text.contains(&format!("experiment {other} ")), "{text}");
        assert!(!text.contains(&format!("experiment {} ", exp())), "{text}");
    }

    #[test]
    fn render_shows_an_aborted_experiment_with_its_failed_checks() {
        let mut p = projection(RoundState::Aborted, &[]);
        p.experiments.get_mut(&exp()).unwrap().aborted =
            Some(horch_core::measure::event::ExperimentAborted {
                reason: "preflight failed".into(),
                failed_checks: vec!["PRE-01".into(), "PRE-02".into()],
            });
        assert_eq!(
            render(&p, None, 2),
            format!(
                "experiment {} ABORTED task task-1 candidates 2 base 0123456789ab\n  \
                 aborted: preflight failed (PRE-01, PRE-02)\n2 torn lines skipped\n",
                exp()
            )
        );
    }

    #[test]
    fn render_skips_a_round_that_has_no_view() {
        let mut p = projection(RoundState::Planned, &[RoundState::Running]);
        p.rounds.clear();
        assert_eq!(
            render(&p, None, 0),
            format!(
                "experiment {} PLANNED task task-1 candidates 2 base 0123456789ab\n",
                exp()
            )
        );
    }
}
