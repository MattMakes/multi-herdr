//! The environment snapshot recorded with an experiment (SEC-02).
//!
//! Never the full environment and never a key: only an allowlist survives,
//! and every kept value is redacted again. The caller passes the variables
//! in; this module never reads the process environment.

use std::collections::BTreeMap;

use crate::measure::redact::{redact, redact_env_value};

/// Kept with their (redacted) values.
const KEEP_VALUE: &[&str] = &["LANG", "LC_ALL", "TERM", "SHELL", "TZ", "HORCH_BALANCE"];
/// Kept as `"set"`: the value is a local path.
const KEEP_PRESENCE: &[&str] = &["HORCH_TEAMMATES_DIR"];
/// What a presence-only variable records.
pub(crate) const PRESENT: &str = "set";

/// `HORCH_<NAME>_BIN`, such as `HORCH_CLAUDE_BIN`.
fn is_bin_override(key: &str) -> bool {
    key.len() > "HORCH__BIN".len() && key.starts_with("HORCH_") && key.ends_with("_BIN")
}

/// The allowlisted part of `vars`.
pub fn env_snapshot(vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    vars.iter()
        .filter_map(|(key, value)| {
            if KEEP_PRESENCE.contains(&key.as_str()) || is_bin_override(key) {
                Some((key.clone(), PRESENT.to_string()))
            } else if KEEP_VALUE.contains(&key.as_str()) {
                let value = redact_env_value(key, value);
                Some((key.clone(), redact(&value).into_owned()))
            } else {
                None
            }
        })
        .collect()
}
