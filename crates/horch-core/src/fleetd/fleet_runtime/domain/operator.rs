#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFacts {
    pub attempt: String,
    pub task: Option<String>,
    pub persona: String,
}
