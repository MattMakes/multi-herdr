#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Open,
    Resolved,
    Superseded,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Resolved => "resolved",
            State::Superseded => "superseded",
        }
    }

    pub fn parse(word: &str) -> Option<State> {
        [State::Open, State::Resolved, State::Superseded]
            .into_iter()
            .find(|s| s.as_str() == word)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub entity: &'a str,
    pub entity_id: &'a str,
    pub text: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub kind: String,
    pub entity: String,
    pub entity_id: String,
    pub state: State,
    pub text: String,
    pub created_at: String,
    pub ended_at: Option<String>,
}

impl Item {
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

    pub fn ended(&self, to: State, now: &str) -> Option<Item> {
        (self.state == State::Open && to != State::Open).then(|| Item {
            state: to,
            ended_at: Some(now.to_string()),
            ..self.clone()
        })
    }
}
