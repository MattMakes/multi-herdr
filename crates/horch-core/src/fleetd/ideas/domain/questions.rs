//! The questions of a run and their owner (`docs/specs/fleetd/design.md`,
//! "Ledger", the `questions` table; `docs/tickets/fleetd-p2/MAP.md`, Q13).
//!
//! Before Phase 6, `feature_id` holds the run id and the owner is the
//! orchestrator of the run.

/// The state of a `questions` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Answered,
    /// Past its time limit, the question went to you (RR-12, Phase 6).
    Escalated,
}

impl State {
    /// The word of the `state` column.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Answered => "answered",
            State::Escalated => "escalated",
        }
    }

    /// The state of a `state` column word.
    pub fn parse(word: &str) -> Option<State> {
        [State::Open, State::Answered, State::Escalated]
            .into_iter()
            .find(|s| s.as_str() == word)
    }
}

/// 1 `questions` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// `<attempt id>/Q<n>`.
    pub id: String,
    pub task_id: String,
    /// The run id before Phase 4.
    pub feature_id: String,
    pub owner: String,
    pub state: State,
    /// The time limit, on the fleet clock.
    pub due_at: i64,
    pub asked_at: String,
    pub answered_at: Option<String>,
}

/// The id of question `n` of attempt `attempt_id`.
pub fn row_id(attempt_id: &str, n: u32) -> String {
    format!("{attempt_id}/Q{n}")
}

/// The number of the next question of an attempt whose rows have the ids
/// `ids`: 1 above the highest number.
pub fn next_n<'a>(attempt_id: &str, ids: impl IntoIterator<Item = &'a str>) -> u32 {
    let prefix = format!("{attempt_id}/Q");
    ids.into_iter()
        .filter_map(|id| id.strip_prefix(&prefix)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// `text`, the words of an agent or of the operator, as data to print: each
/// control character but a newline becomes a space.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_n_is_1_above_the_highest_question_of_the_attempt() {
        assert_eq!(next_n("A1", []), 1);
        assert_eq!(next_n("A1", ["A1/Q1", "A1/Q3", "A2/Q9", "A1/Qx"]), 4);
        assert_eq!(row_id("A1", 4), "A1/Q4");
    }

    #[test]
    fn printable_removes_control_characters_but_keeps_lines() {
        assert_eq!(printable("a\u{1b}[2Jb\r\nc\td"), "a [2Jb \nc d");
    }

    #[test]
    fn each_state_word_parses_back() {
        for s in [State::Open, State::Answered, State::Escalated] {
            assert_eq!(State::parse(s.as_str()), Some(s));
        }
        assert_eq!(State::parse("closed"), None);
    }
}
