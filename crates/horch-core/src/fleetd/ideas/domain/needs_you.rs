//! The needs-you item (FM-44).
//!
//! Each item has a stable key, `<kind>:<entity id>`, and a state. A producer
//! writes by key, so 1 thing has 1 item. Only an `open` item is shown. An
//! item that is not open is removed `retention_days` after it ended.

/// The state of a needs-you item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    /// Its cause ended: an answer, an approval, a requeue.
    Resolved,
    /// A newer item of the same row replaced it.
    Superseded,
}

impl State {
    /// The word of the `state` column.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Resolved => "resolved",
            State::Superseded => "superseded",
        }
    }

    /// The state of a `state` column word.
    pub fn parse(word: &str) -> Option<State> {
        [State::Open, State::Resolved, State::Superseded]
            .into_iter()
            .find(|s| s.as_str() == word)
    }
}

/// What a producer writes: the item of `key`, open, with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub entity: &'a str,
    pub entity_id: &'a str,
    pub text: &'a str,
}

/// 1 needs-you item, as its row holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub kind: String,
    pub entity: String,
    pub entity_id: String,
    pub state: State,
    pub text: String,
    pub created_at: String,
    /// When it stopped being open; `None` while it is open.
    pub ended_at: Option<String>,
}

impl Item {
    /// The item of `key` after a producer writes `w` at `now`, or `None`
    /// when the write changes nothing.
    ///
    /// An open item keeps its `created_at`. An item that opens again starts
    /// a new open period at `now`.
    pub fn written(old: Option<&Item>, w: &Write<'_>, now: &str) -> Option<Item> {
        let created_at = match old {
            Some(o) if o.state == State::Open => o.created_at.clone(),
            _ => now.to_string(),
        };
        let new = Item {
            key: w.key.to_string(),
            kind: w.kind.to_string(),
            entity: w.entity.to_string(),
            entity_id: w.entity_id.to_string(),
            state: State::Open,
            text: w.text.to_string(),
            created_at,
            ended_at: None,
        };
        (old != Some(&new)).then_some(new)
    }

    /// The item after it ends in `to` at `now`, or `None` when it is not
    /// open.
    pub fn ended(&self, to: State, now: &str) -> Option<Item> {
        (self.state == State::Open && to != State::Open).then(|| Item {
            state: to,
            ended_at: Some(now.to_string()),
            ..self.clone()
        })
    }
}
