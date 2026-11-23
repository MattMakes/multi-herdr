use super::retry::Outcome;

pub const CONTEXT_LIMIT: u64 = 256_000;

pub fn reached(tokens: u64) -> bool {
    tokens >= CONTEXT_LIMIT
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End<'a> {
    pub outcome: Option<&'a str>,
    pub head: Option<&'a str>,
}

impl End<'_> {
    fn at_limit(&self) -> bool {
        self.outcome == Some(Outcome::ContextLimit.as_str())
    }
}

pub fn loops(previous: Option<&End<'_>>, this: &End<'_>) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if !previous.at_limit() || !this.at_limit() {
        return false;
    }
    matches!((previous.head, this.head), (Some(a), Some(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Option<&str> = Some("context_limit");

    fn end<'a>(outcome: Option<&'a str>, head: Option<&'a str>) -> End<'a> {
        End { outcome, head }
    }

    #[test]
    fn the_limit_is_256000_tokens() {
        assert_eq!(CONTEXT_LIMIT, 256_000);
        assert!(!reached(255_999));
        assert!(reached(256_000));
        assert!(reached(300_000));
    }

    #[test]
    fn two_limits_at_the_same_head_loop() {
        let this = end(LIMIT, Some("abc"));
        assert!(loops(Some(&end(LIMIT, Some("abc"))), &this));
    }

    #[test]
    fn a_new_commit_between_the_limits_is_no_loop() {
        let this = end(LIMIT, Some("def"));
        assert!(!loops(Some(&end(LIMIT, Some("abc"))), &this));
    }

    #[test]
    fn the_first_limit_is_no_loop() {
        assert!(!loops(None, &end(LIMIT, Some("abc"))));
        let crashed = end(Some("crashed"), Some("abc"));
        assert!(!loops(Some(&crashed), &end(LIMIT, Some("abc"))));
    }

    #[test]
    fn an_unknown_head_is_no_loop() {
        assert!(!loops(Some(&end(LIMIT, None)), &end(LIMIT, Some("abc"))));
        assert!(!loops(Some(&end(LIMIT, Some("abc"))), &end(LIMIT, None)));
        assert!(!loops(Some(&end(LIMIT, None)), &end(LIMIT, None)));
    }
}
