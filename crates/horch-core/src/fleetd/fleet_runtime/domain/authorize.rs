//! The authorization decision of a request: authorized with the text of
//! `auth`, or refused with a reason, from the facts of the request alone.
//!
//! Requirements: SR-5, SR-8, FM-32. Ticket 09 of fleetd-p5 fills it.
//!
//! The rules, in this order:
//! 1. A kind that fleetd does not know is refused.
//! 2. A `worker` kind is refused: it is a file of its attempt.
//! 3. A caller that is not the user of the ledger, or an unknown caller, is
//!    refused (SR-5).
//! 4. An `operator` kind with no valid token is refused (SR-8).
//! 5. On the channel, a peer that is a fleet process (FR-10: a process of
//!    an attempt that is live or that ended a short time ago), or a peer
//!    whose fact is unknown, is refused (SR-8). No attempt has operator rights: Phase 6
//!    removed the orchestrator seat (FM-32; ticket 44 of fleetd-p6).
//!
//! Else the request is authorized. A guard whose fact is unknown does not
//! hold, so an unknown fact never authorizes.

use super::operator::AttemptFacts;
use super::requests::{Class, Kind};

/// The facts of 1 request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestFacts {
    /// The wire name of the kind.
    pub kind: String,
    /// Where the request came from.
    pub source: Source,
    /// The peer user of the channel, or the owner of the request file.
    /// `None` when the fact is not supported or fails to read.
    pub caller: Option<String>,
    /// The user that owns the ledger. `None` when the fact fails to read.
    pub ledger_user: Option<String>,
    /// The request carries the operator token and it is equal to the token
    /// of `run/operator.token`. `false` when the token is absent, is not
    /// equal, or fails to read.
    pub token_valid: bool,
}

/// The source of a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The command channel, with the fact of the peer process.
    Channel { peer: PeerProcess },
    /// A file of the request folder.
    RequestFile,
}

/// Is the peer process of the channel a fleet process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerProcess {
    /// The peer is not a fleet process.
    Outside,
    /// The peer is a process of each of these attempts, as the ledger
    /// records them.
    Inside(Vec<AttemptFacts>),
    /// A fact of the peer is not supported or fails to read.
    Unknown,
}

/// The decision on 1 request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The text of the `auth` column: how fleetd authorized the request.
    Authorized { auth: String },
    /// The reason of the refusal.
    Refused { reason: String },
}

/// The decision on the request of `facts`.
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

/// SR-5: the caller is the user of the ledger, and both facts are known.
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

/// SR-8, FR-10: only a peer that is not a fleet process. A fleet process is
/// refused, whatever its task or persona.
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

    /// A request that passes each rule: an operator kind on the channel,
    /// from the user of the ledger, with a valid token, from a peer outside
    /// each live attempt.
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
        // An attempt with a null task.
        let facts = RequestFacts {
            source: Source::Channel {
                peer: PeerProcess::Inside(vec![attempt("A-4", None, "opus-hardening")]),
            },
            ..channel()
        };
        assert!(refusal(&facts).contains("fleet process of attempt A-4"));
        // 2 attempts hold the peer: the reason names the first.
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
        // No attempt named: the fact says nothing, so it does not hold.
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
