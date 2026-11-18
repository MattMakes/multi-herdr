//! The request kinds of Phase 6: the idea kinds of class `operator`, and
//! `idea_note`, a file of class `worker` that fleetd refuses on the channel
//! and in a request file (docs/tickets/fleetd-p6/06-kinds.md; SR-8, FR-10).

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{parse_request, Request};
use horch_core::fleetd::fleet_runtime::domain::authorize::{
    authorize, Decision, PeerProcess, RequestFacts, Source,
};
use horch_core::fleetd::fleet_runtime::domain::requests::{
    Class, DirectTicket, IdeaAdd, IdeaInput, Kind, RequestError, RequestId,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";
const MAX_BYTES: u64 = 64 * 1024;

/// The operator facts of a request of `kind` from `source`: the caller is
/// the user of the ledger and the token is valid.
fn operator_facts(kind: &str, source: Source) -> RequestFacts {
    RequestFacts {
        kind: kind.into(),
        source,
        caller: Some("me".into()),
        ledger_user: Some("me".into()),
        token_valid: true,
        orchestrator_seats: Vec::new(),
    }
}

/// `add` as an `idea_add` request line, parsed back as fleetd parses it.
fn through_the_wire(add: &IdeaAdd) -> IdeaAdd {
    let payload: Map<String, Value> = add
        .fields()
        .into_iter()
        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
        .collect();
    let request = Request {
        request_id: RequestId::parse(ID).unwrap(),
        kind: Kind::IdeaAdd,
        payload,
        token: Some("t".into()),
    };
    let parsed = parse_request(request.to_line().as_bytes(), MAX_BYTES).unwrap();
    assert_eq!(parsed.kind, Kind::IdeaAdd);
    let fields: Vec<(&str, &str)> = parsed
        .payload
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str().unwrap()))
        .collect();
    IdeaAdd::from_fields(&fields).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn absolute(name: &str) -> String {
    std::env::temp_dir()
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn each_idea_kind_is_operator() {
    let operator = [
        ("idea_add", Kind::IdeaAdd),
        ("idea_amend", Kind::IdeaAmend),
        ("idea_approve", Kind::IdeaApprove),
        ("idea_revise", Kind::IdeaRevise),
        ("idea_park", Kind::IdeaPark),
        ("idea_cancel", Kind::IdeaCancel),
    ];
    for (name, kind) in operator {
        assert_eq!(Kind::parse(name), Ok(kind), "{name}");
        assert_eq!(kind.as_str(), name);
        assert_eq!(kind.class(), Class::Operator, "{name}");
        assert!(Kind::ALL.contains(&kind), "{name} is in Kind::ALL");
        let decision = authorize(&operator_facts(name, Source::RequestFile));
        assert!(
            matches!(decision, Decision::Authorized { .. }),
            "{name}: {decision:?}"
        );
    }
    // FR-10: the approval is an operator request, so it needs the token.
    let mut facts = operator_facts("idea_approve", Source::RequestFile);
    facts.token_valid = false;
    assert!(matches!(authorize(&facts), Decision::Refused { .. }));
}

#[test]
fn idea_note_on_the_channel_is_refused() {
    assert_eq!(Kind::parse("idea_note"), Ok(Kind::IdeaNote));
    assert_eq!(Kind::IdeaNote.class(), Class::Worker);
    assert!(Kind::ALL.contains(&Kind::IdeaNote));
    let workers: Vec<&str> = Kind::ALL
        .iter()
        .filter(|k| k.class() == Class::Worker)
        .map(|k| k.as_str())
        .collect();
    assert_eq!(workers, ["progress", "result", "ask", "idea_note"]);
    let sources = [
        Source::Channel {
            peer: PeerProcess::Outside,
        },
        Source::RequestFile,
    ];
    for source in sources {
        match authorize(&operator_facts("idea_note", source.clone())) {
            Decision::Refused { reason } => {
                assert!(reason.contains("`idea_note` is a worker kind"), "{reason}");
            }
            other => panic!("{source:?}: idea_note is not refused: {other:?}"),
        }
    }
}

#[test]
fn idea_add_payload_has_direct_and_to() {
    let path = absolute("ticket.md");
    let sha = sha256_hex(b"# 01: A ticket\n");
    let add = IdeaAdd {
        idea: IdeaInput::Direct(DirectTicket::new(&path, &sha).unwrap()),
        to: Some("F-0002".into()),
    };
    let mut keys: Vec<&str> = add.fields().into_iter().map(|(k, _)| k).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["direct", "sha256", "to"]);
    assert_eq!(through_the_wire(&add), add);

    let text = IdeaAdd {
        idea: IdeaInput::Text("make the board faster".into()),
        to: None,
    };
    let keys: Vec<&str> = text.fields().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["text"]);
    assert_eq!(through_the_wire(&text), text);

    let joined = IdeaAdd {
        idea: IdeaInput::Text("a second idea".into()),
        to: Some("F-0002".into()),
    };
    assert_eq!(through_the_wire(&joined), joined);

    for bad in [
        vec![],
        vec![("to", "F-0002")],
        vec![("text", "x"), ("direct", path.as_str()), ("sha256", &sha)],
        vec![("text", "x"), ("to", "")],
        vec![("text", "x"), ("from", "y")],
    ] {
        assert!(
            matches!(IdeaAdd::from_fields(&bad), Err(RequestError::Malformed(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn idea_add_direct_payload_has_the_sha256_of_the_ticket() {
    let path = absolute("ticket.md");
    let ticket = b"# 01: A ticket\n\n**Check:** `true`\n";
    let sha = sha256_hex(ticket);
    let direct = DirectTicket::new(&path, &sha).unwrap();
    assert_eq!(direct.path(), path);
    assert_eq!(direct.sha256(), sha);
    let add = IdeaAdd {
        idea: IdeaInput::Direct(direct),
        to: None,
    };
    let fields = add.fields();
    assert!(fields.contains(&("direct", path.as_str())), "{fields:?}");
    assert!(fields.contains(&("sha256", sha.as_str())), "{fields:?}");

    let refused = |fields: &[(&str, &str)]| {
        matches!(
            IdeaAdd::from_fields(fields),
            Err(RequestError::Malformed(_))
        )
    };
    assert!(refused(&[("direct", &path)]), "--direct needs its sha256");
    assert!(
        refused(&[("text", "x"), ("sha256", &sha)]),
        "a sha256 needs --direct"
    );
    assert!(
        refused(&[("direct", "ticket.md"), ("sha256", &sha)]),
        "relative"
    );
    assert!(refused(&[("direct", ""), ("sha256", &sha)]), "empty path");
    let upper = sha.to_ascii_uppercase();
    let short = &sha[..63];
    let long = format!("{sha}0");
    let not_hex = format!("{}g", &sha[..63]);
    for bad in [upper.as_str(), short, long.as_str(), not_hex.as_str(), ""] {
        assert!(DirectTicket::new(&path, bad).is_err(), "{bad:?}");
        assert!(refused(&[("direct", &path), ("sha256", bad)]), "{bad:?}");
    }
}
