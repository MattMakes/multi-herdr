use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::domain::authorize::{
    authorize, Decision, PeerProcess, RequestFacts, Source,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::operator::AttemptFacts;

fn old_seat(attempt: &str) -> AttemptFacts {
    AttemptFacts {
        attempt: attempt.into(),
        task: None,
        persona: "orchestrator".into(),
    }
}

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
                    reason.contains("the peer process is a fleet process of attempt A-7"),
                    "{persona}: {reason}"
                );
                assert!(reason.contains(persona), "{persona}: {reason}");
            }
            other => panic!("{persona}: an old seat has no operator right: {other:?}"),
        }
    }
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
