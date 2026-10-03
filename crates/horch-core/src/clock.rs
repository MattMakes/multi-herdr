//! The one clock horch reads.
//!
//! Everything that asks "what time is it" goes through [`now`], so a test can
//! pin the time with `HORCH_NOW=<RFC 3339>` and get the same answer from the
//! ledger, the telemetry collector, the quota states and the spawn gate. A
//! pinned clock is announced once on stderr, so it cannot go unnoticed in a
//! real run.
//!
//! This module does not read `HORCH_NOW` itself. The binary's bootstrap reads
//! it into `RuntimeContext::settings` and passes it to [`install`] once.

use std::sync::{Once, OnceLock};

use chrono::{DateTime, SecondsFormat, Utc};

/// What [`install`] was given: the pinned time, or the value that did not
/// parse.
#[derive(Debug)]
enum Pin {
    At(DateTime<Utc>),
    Unparsable(String),
}

static PIN: OnceLock<Pin> = OnceLock::new();

/// Pin the clock for this process: `now` is `HORCH_NOW` parsed, and
/// `unparsable` is its raw value when it did not parse. The first call wins;
/// later calls change nothing. Without a call the clock is the system's.
pub fn install(now: Option<DateTime<Utc>>, unparsable: Option<&str>) {
    let pin = match (now, unparsable) {
        (Some(t), _) => Pin::At(t),
        (None, Some(raw)) => Pin::Unparsable(raw.to_string()),
        (None, None) => return,
    };
    let _ = PIN.set(pin);
}

/// The current time, or `HORCH_NOW` when it is set.
pub fn now() -> DateTime<Utc> {
    pinned().unwrap_or_else(Utc::now)
}

/// The installed `HORCH_NOW`. An unparsable value is ignored with a warning
/// rather than silently treated as "now".
fn pinned() -> Option<DateTime<Utc>> {
    let pin = PIN.get()?;
    static ANNOUNCE: Once = Once::new();
    match pin {
        Pin::At(t) => {
            ANNOUNCE.call_once(|| eprintln!("horch: HORCH_NOW is set"));
            Some(*t)
        }
        Pin::Unparsable(raw) => {
            ANNOUNCE.call_once(|| {
                eprintln!("horch: HORCH_NOW='{raw}' is not an RFC 3339 time; ignoring it")
            });
            None
        }
    }
}

/// The ledger's timestamp format: `2026-09-28T17:40:02Z`. Lexicographic order
/// equals chronological order.
pub fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// [`stamp`] of [`now`].
pub fn now_stamp() -> String {
    stamp(now())
}

/// Parse an RFC 3339 time, or a bare `YYYY-MM-DD` date (midnight UTC).
pub fn parse(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return Some(t.with_timezone(&Utc));
    }
    let date = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc())
}

/// Seconds since the Unix epoch, as a UTC time. Codex reports resets this way.
pub fn from_epoch(secs: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(secs, 0)
}

/// Milliseconds since the Unix epoch. OpenCode and pi stamp messages this way.
pub fn from_epoch_ms(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_round_trip_and_sort_chronologically() {
        let t = parse("2026-09-28T17:40:02Z").unwrap();
        assert_eq!(stamp(t), "2026-09-28T17:40:02Z");
        let later = parse("2026-09-28T17:40:02.900+00:00").unwrap();
        assert!(later > t);
        assert_eq!(stamp(later), "2026-09-28T17:40:02Z", "sub-seconds drop");
        assert!(stamp(t) < stamp(parse("2026-10-01T00:00:00Z").unwrap()));
    }

    #[test]
    fn dates_and_offsets_normalise_to_utc() {
        assert_eq!(stamp(parse("2026-09-24").unwrap()), "2026-09-24T00:00:00Z");
        assert_eq!(
            stamp(parse("2026-10-02T07:00:00-07:00").unwrap()),
            "2026-10-02T14:00:00Z"
        );
        assert!(parse("next tuesday").is_none());
    }

    #[test]
    fn epoch_seconds_and_millis_convert() {
        assert_eq!(
            stamp(from_epoch(1_791_054_949).unwrap()),
            "2026-10-03T19:15:49Z"
        );
        assert_eq!(
            stamp(from_epoch_ms(1_790_600_005_000).unwrap()),
            stamp(from_epoch(1_790_600_005).unwrap())
        );
    }
}
