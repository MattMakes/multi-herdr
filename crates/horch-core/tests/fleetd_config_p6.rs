use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::domain::config::{FleetConfig, Weekday, WorkingHours};

#[test]
fn phase6_keys_have_the_values_of_the_design() {
    let c = config_file::parse("").unwrap();
    assert_eq!(c.idle_stop_s, 1800);
    assert_eq!(c.park_after_days, 7);
    assert_eq!(c.reminder_every_s, 3600);
    assert_eq!(
        c.working_hours,
        WorkingHours::parse("Mon-Fri 09:00-18:00").unwrap()
    );
    assert_eq!(c, FleetConfig::default());

    let c = config_file::parse(
        "idle_stop_s = 60\npark_after_days = 365\nreminder_every_s = 600\n\
         working_hours = \"Sat-Sun 10:30-12:00\"\n",
    )
    .unwrap();
    assert_eq!(c.idle_stop_s, 60);
    assert_eq!(c.park_after_days, 365);
    assert_eq!(c.reminder_every_s, 600);
    assert_eq!(
        c.working_hours,
        WorkingHours::parse("Sat-Sun 10:30-12:00").unwrap()
    );
}

#[test]
fn each_phase6_key_out_of_range_is_refused_with_its_name() {
    for (text, key) in [
        ("idle_stop_s = 59", "idle_stop_s"),
        ("idle_stop_s = 86401", "idle_stop_s"),
        ("park_after_days = 0", "park_after_days"),
        ("park_after_days = 366", "park_after_days"),
        ("reminder_every_s = 599", "reminder_every_s"),
        ("reminder_every_s = 86401", "reminder_every_s"),
    ] {
        let err = format!("{:#}", config_file::parse(text).unwrap_err());
        assert!(err.contains(key), "{text}: {err}");
    }
    for text in [
        "idle_stop_s = 86400",
        "park_after_days = 1",
        "reminder_every_s = 86400",
    ] {
        config_file::parse(text).unwrap();
    }
}

#[test]
fn working_hours_parses_days_and_hours() {
    let h = WorkingHours::parse("Mon-Fri 09:00-18:00").unwrap();
    assert!(h.contains(Weekday::Mon, 9 * 60));
    assert!(h.contains(Weekday::Wed, 12 * 60 + 30));
    assert!(h.contains(Weekday::Fri, 17 * 60 + 59));
    assert!(!h.contains(Weekday::Fri, 18 * 60));
    assert!(!h.contains(Weekday::Mon, 8 * 60 + 59));
    assert!(!h.contains(Weekday::Sat, 12 * 60));
    assert!(!h.contains(Weekday::Sun, 12 * 60));

    let h = WorkingHours::parse("Fri-Mon 22:00-23:30").unwrap();
    assert!(h.contains(Weekday::Sun, 22 * 60));
    assert!(h.contains(Weekday::Mon, 23 * 60 + 29));
    assert!(!h.contains(Weekday::Tue, 22 * 60));

    let h = WorkingHours::parse("Wed-Wed 00:00-23:59").unwrap();
    assert!(h.contains(Weekday::Wed, 0));
    assert!(!h.contains(Weekday::Thu, 0));
}

#[test]
fn working_hours_with_a_bad_form_is_refused() {
    for bad in [
        "",
        "Mon-Fri",
        "09:00-18:00",
        "Mon 09:00-18:00",
        "mon-fri 09:00-18:00",
        "Mon-Fri 9:00-18:00",
        "Mon-Fri 09:00-24:00",
        "Mon-Fri 09:60-18:00",
        "Mon-Fri 18:00-09:00",
        "Mon-Fri 09:00-09:00",
        "Mon-Xyz 09:00-18:00",
        "Mon-Fri  09:00-18:00",
        "Mon-Fri 09:00-18:00 extra",
    ] {
        assert!(WorkingHours::parse(bad).is_err(), "{bad:?} is accepted");
        let text = format!("working_hours = {bad:?}\n");
        let err = format!("{:#}", config_file::parse(&text).unwrap_err());
        assert!(err.contains("working_hours"), "{bad:?}: {err}");
    }
    let err = format!(
        "{:#}",
        config_file::parse("working_hours = 9\n").unwrap_err()
    );
    assert!(err.contains("working_hours"), "{err}");
}
