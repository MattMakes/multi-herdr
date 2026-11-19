//! Phase 6 removes the orchestrator seat: no agent has operator rights
//! (SR-8, FM-32; ticket 44 of fleetd-p6). An attempt with a null task whose
//! persona is `orchestrator`, the old seat, is a live attempt like each
//! other, so its operator request on the channel is refused. The key
//! `orchestrator_seats` of `fleet.toml` gives 1 warning line and no right
//! (Q19 of `docs/tickets/fleetd-p6/MAP.md`).

use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::domain::authorize::{
    authorize, Decision, PeerProcess, RequestFacts, Source,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::operator::AttemptFacts;

/// A live attempt of the old seat: a null task and the persona
/// `orchestrator`.
fn old_seat(attempt: &str) -> AttemptFacts {
    AttemptFacts {
        attempt: attempt.into(),
        task: None,
        persona: "orchestrator".into(),
    }
}

/// An operator request of `ledger_gc` on the channel from a peer in the
/// live attempts `attempts`: the caller is the user of the ledger and the
/// token is valid.
fn channel_facts(attempts: Vec<AttemptFacts>) -> RequestFacts {
    RequestFacts {
        kind: "ledger_gc".into(),
        source: Source::Channel {
            peer: PeerProcess::Inside(attempts),
        },
        caller: Some("me".into()),
        ledger_user: Some("me".into()),
        token_valid: true,
    }
}

#[test]
fn orchestrator_seat_has_no_operator_right() {
    let personas = ["orchestrator", "orchestrator-codex"];
    for persona in personas {
        let seat = AttemptFacts {
            persona: persona.into(),
            ..old_seat("A-7")
        };
        match authorize(&channel_facts(vec![seat])) {
            Decision::Refused { reason } => {
                assert!(
                    reason.contains("the peer process is in live attempt A-7"),
                    "{persona}: {reason}"
                );
                assert!(reason.contains(persona), "{persona}: {reason}");
            }
            other => panic!("{persona}: an old seat has no operator right: {other:?}"),
        }
    }
    // 2 attempts of the old seat prove nothing more than 1.
    let two = channel_facts(vec![old_seat("A-1"), old_seat("A-2")]);
    assert!(
        matches!(authorize(&two), Decision::Refused { .. }),
        "{two:?}"
    );
}

#[test]
fn orchestrator_seats_key_gives_1_warning_line_and_no_right() {
    let text = "orchestrator_seats = [\"orchestrator\"]\n";
    let (config, warning) = config_file::parse_with_warning(text).unwrap();
    assert_eq!(config, FleetConfig::default(), "the key sets nothing");
    let warning = warning.expect("the retired key gives a warning");
    assert!(warning.contains("orchestrator_seats"), "{warning}");
    assert!(!warning.contains('\n'), "1 line: {warning:?}");
    assert_eq!(config_file::parse(text).unwrap(), FleetConfig::default());

    let (_, none) = config_file::parse_with_warning("pass_period_s = 1\n").unwrap();
    assert_eq!(none, None, "a file without the key gives no warning");
}
