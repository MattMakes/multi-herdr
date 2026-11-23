use super::operator::AttemptFacts;
use super::requests::{Class, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestFacts {
    pub kind: String,
    pub source: Source,
    pub caller: Option<String>,
    pub ledger_user: Option<String>,
    pub token_valid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Channel { peer: PeerProcess },
    RequestFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerProcess {
    Outside,
    Inside(Vec<AttemptFacts>),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Authorized { auth: String },
    Refused { reason: String },
}

pub fn authorize(facts: &RequestFacts) -> Decision {
    match decide(facts) {
        Ok(auth) => Decision::Authorized { auth },
        Err(reason) => Decision::Refused { reason },
    }
}

fn decide(facts: &RequestFacts) -> Result<String, String> {
    let kind = Kind::parse(&facts.kind).map_err(|e| e.to_string())?;
    if kind.class() == Class::Worker {
        return Err(format!(
            "`{}` is a worker kind: a worker kind is a file of its attempt",
            kind.as_str()
        ));
    }
    check_caller(facts)?;
    if !facts.token_valid {
        return Err("an operator kind needs the valid operator token of run/operator.token".into());
    }
    match &facts.source {
        Source::RequestFile => Ok("token; request file".into()),
        Source::Channel { peer } => check_peer(peer),
    }
}

fn check_caller(facts: &RequestFacts) -> Result<(), String> {
    let Some(ledger_user) = &facts.ledger_user else {
        return Err("the user of the ledger is unknown".into());
    };
    match &facts.caller {
        None => Err("the caller is unknown".into()),
        Some(caller) if caller != ledger_user => Err(format!(
            "the caller {caller} is not the user of the ledger ({ledger_user})"
        )),
        Some(_) => Ok(()),
    }
}

fn check_peer(peer: &PeerProcess) -> Result<String, String> {
    match peer {
        PeerProcess::Outside => Ok("token; peer outside each live attempt".into()),
        PeerProcess::Unknown => {
            Err("the peer process is unknown, so the peer process check does not hold".into())
        }
        PeerProcess::Inside(attempts) => match attempts.first() {
            None => Err(
                "the peer process is a fleet process of an attempt that the fact does not name"
                    .into(),
            ),
            Some(a) => Err(format!(
                "the peer process is a fleet process of attempt {} of persona {}",
                a.attempt, a.persona
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "uid 501";

    fn attempt(id: &str, task: Option<&str>, persona: &str) -> AttemptFacts {
        AttemptFacts {
            attempt: id.to_string(),
            task: task.map(str::to_string),
            persona: persona.to_string(),
        }
    }

    fn channel() -> RequestFacts {
        RequestFacts {
            kind: "task_cancel".to_string(),
            source: Source::Channel {
                peer: PeerProcess::Outside,
            },
            caller: Some(USER.to_string()),
            ledger_user: Some(USER.to_string()),
            token_valid: true,
        }
    }

    fn request_file() -> RequestFacts {
        RequestFacts {
            source: Source::RequestFile,
            ..channel()
        }
    }

    fn refusal(facts: &RequestFacts) -> String {
        match authorize(facts) {
            Decision::Refused { reason } => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn auth(facts: &RequestFacts) -> String {
        match authorize(facts) {
            Decision::Authorized { auth } => auth,
            other => panic!("expected an authorization, got {other:?}"),
        }
    }

    #[test]
    fn worker_kind_is_refused() {
        for kind in ["progress", "result", "ask"] {
            for source in [channel(), request_file()] {
                let facts = RequestFacts {
                    kind: kind.to_string(),
                    ..source
                };
                let reason = refusal(&facts);
                assert!(reason.contains("worker kind"), "{reason}");
                assert!(reason.contains(kind), "{reason}");
            }
        }
    }

    #[test]
    fn unknown_kind_is_refused() {
        let facts = RequestFacts {
            kind: "idea_show".to_string(),
            ..channel()
        };
        let reason = refusal(&facts);
        assert!(
            reason.contains("unknown request kind `idea_show`"),
            "{reason}"
        );
    }

    #[test]
    fn other_user_on_the_channel_is_refused() {
        let facts = RequestFacts {
            caller: Some("uid 502".to_string()),
            ..channel()
        };
        let reason = refusal(&facts);
        assert!(reason.contains("uid 502"), "{reason}");
        assert!(reason.contains("not the user of the ledger"), "{reason}");
    }

    #[test]
    fn unknown_peer_user_is_refused() {
        let facts = RequestFacts {
            caller: None,
            ..channel()
        };
        assert!(refusal(&facts).contains("caller is unknown"));
        let facts = RequestFacts {
            ledger_user: None,
            ..channel()
        };
        assert!(refusal(&facts).contains("user of the ledger is unknown"));
    }

    #[test]
    fn request_file_of_another_owner_is_refused() {
        let facts = RequestFacts {
            caller: Some("uid 0".to_string()),
            ..request_file()
        };
        let reason = refusal(&facts);
        assert!(reason.contains("uid 0"), "{reason}");
        assert!(reason.contains("not the user of the ledger"), "{reason}");
    }

    #[test]
    fn operator_kind_without_token_is_refused_and_names_the_token() {
        for source in [channel(), request_file()] {
            let facts = RequestFacts {
                token_valid: false,
                ..source
            };
            let reason = refusal(&facts);
            assert!(reason.contains("operator token"), "{reason}");
            assert!(reason.contains("run/operator.token"), "{reason}");
        }
    }

    #[test]
    fn operator_kind_from_a_live_attempt_is_refused() {
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Inside(vec![attempt("A-3", Some("T-9"), "opus-hardening")]),
            },
            ..channel()
        };
        let reason = refusal(&facts);
        assert!(reason.contains("fleet process of attempt A-3"), "{reason}");
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Inside(vec![attempt("A-4", None, "opus-hardening")]),
            },
            ..channel()
        };
        assert!(refusal(&facts).contains("fleet process of attempt A-4"));
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Inside(vec![
                    attempt("A-1", None, "orchestrator"),
                    attempt("A-3", Some("T-9"), "opus-hardening"),
                ]),
            },
            ..channel()
        };
        assert!(refusal(&facts).contains("fleet process of attempt A-1"));
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Inside(Vec::new()),
            },
            ..channel()
        };
        refusal(&facts);
    }

    #[test]
    fn operator_kind_with_unknown_peer_process_is_refused() {
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Unknown,
            },
            ..channel()
        };
        let reason = refusal(&facts);
        assert!(reason.contains("peer process is unknown"), "{reason}");
    }

    #[test]
    fn request_file_with_valid_token_is_authorized() {
        assert_eq!(auth(&request_file()), "token; request file");
        assert_eq!(auth(&channel()), "token; peer outside each live attempt");
    }
}
