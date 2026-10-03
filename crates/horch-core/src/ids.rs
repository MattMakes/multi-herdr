//! Typed identities.
//!
//! Every id the fleet passes around is a string on disk and on the wire, and
//! most of them still are in memory. These newtypes are where that ends: a
//! value of one of these types has been validated once, at the boundary, and
//! cannot be confused with an id of another kind. The serialized form is the
//! bare string, so a newtype field reads and writes exactly the string it replaces
//! (`serde(try_from, into)` rather than `transparent`, which cannot validate).

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};

/// Why a string was rejected as an id. `kind` is the id type's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    Empty {
        kind: &'static str,
    },
    ControlChar {
        kind: &'static str,
    },
    /// A `RoleName` with `/`, `\` or `..`.
    PathLike {
        kind: &'static str,
        value: String,
    },
    /// Not `<workspace>:<role>`.
    BadWorkerId {
        value: String,
    },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { kind } => write!(f, "{kind} must not be empty"),
            Self::ControlChar { kind } => write!(f, "{kind} must not contain control characters"),
            Self::PathLike { kind, value } => {
                write!(f, "{kind} '{value}' must not contain '/', '\\' or '..'")
            }
            Self::BadWorkerId { value } => {
                write!(f, "WorkerId '{value}' must be '<workspace>:<role>'")
            }
        }
    }
}

impl std::error::Error for IdError {}

/// The rule every id shares: non-empty, and nothing a terminal or a log line
/// would interpret.
fn validate_plain(kind: &'static str, value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if value.chars().any(char::is_control) {
        return Err(IdError::ControlChar { kind });
    }
    Ok(())
}

/// Any UUID, or a legacy hand-made id such as `rec-o1` (OD7): the plain rule.
fn validate_execution_id(kind: &'static str, value: &str) -> Result<(), IdError> {
    validate_plain(kind, value)
}

/// A role names a mailbox file and a pane, so it must stay one path segment.
fn validate_role_name(kind: &'static str, value: &str) -> Result<(), IdError> {
    validate_plain(kind, value)?;
    if value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(IdError::PathLike {
            kind,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// `<workspace>:<role>`, with exactly one `:` between two non-empty parts.
fn validate_worker_id(kind: &'static str, value: &str) -> Result<(), IdError> {
    validate_plain(kind, value)?;
    match value.split_once(':') {
        Some((ws, role)) if !ws.is_empty() && !role.is_empty() && !role.contains(':') => Ok(()),
        _ => Err(IdError::BadWorkerId {
            value: value.to_string(),
        }),
    }
}

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, $validate:path) => {
        $(#[$meta])*
        #[derive(
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                $validate(stringify!($name), &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
            }
        }
    };
}

string_id!(
    /// One run of one worker or orchestrator: a ledger record id. New ids are
    /// UUIDv7; legacy records carry v4 UUIDs or hand-made ids such as `rec-o1`.
    ExecutionId,
    validate_execution_id
);
string_id!(TaskId, validate_plain);
string_id!(
    /// An agent CLI's own session handle, the one `--resume` takes.
    SessionId,
    validate_plain
);
string_id!(
    /// A worker within the fleet: `<workspace>:<role>`.
    WorkerId,
    validate_worker_id
);
string_id!(
    /// A worker's role in its workspace, such as `opus-2`.
    RoleName,
    validate_role_name
);
string_id!(SkillId, validate_plain);
string_id!(ModelId, validate_plain);
string_id!(
    /// A `teammates/<name>.md` roster entry.
    TeammateName,
    validate_plain
);
string_id!(PaneId, validate_plain);
string_id!(WorkspaceId, validate_plain);
string_id!(ExperimentId, validate_plain);
string_id!(RoundId, validate_plain);
string_id!(EventId, validate_plain);
string_id!(JudgmentId, validate_plain);

impl WorkerId {
    /// `<workspace>:<role>`. Both parts are validated already; a herdr
    /// workspace id has no `:`, so the result is a well-formed `WorkerId`.
    pub fn new_for(workspace: &WorkspaceId, role: &RoleName) -> Self {
        Self(format!("{workspace}:{role}"))
    }
}

macro_rules! mintable {
    ($($name:ident),*) => {$(
        impl $name {
            /// Mint a fresh UUIDv7 id timestamped `at`.
            pub fn mint(at: DateTime<Utc>) -> Self {
                Self(mint_v7(at).to_string())
            }
        }
    )*};
}

mintable!(ExecutionId, EventId, ExperimentId, RoundId, JudgmentId);

/// A UUIDv7 for `at`, built by hand from v4 randomness (OD7) so the uuid
/// crate's feature set stays as it is.
///
/// Bytes 0..6 hold the big-endian Unix millisecond timestamp, so ids sort by
/// time; the version nibble is 7 and the RFC 4122 variant bits are kept from
/// the v4 source. The remaining 74 bits are random.
pub fn mint_v7(at: DateTime<Utc>) -> uuid::Uuid {
    let mut bytes = *uuid::Uuid::new_v4().as_bytes();
    // A pre-1970 instant has no v7 encoding; clamp rather than wrap.
    let ms = at.timestamp_millis().max(0) as u64;
    bytes[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    uuid::Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_02_ids_validate() {
        assert!(TaskId::new("").is_err());
        assert!(SessionId::new("a\nb").is_err());
        assert!(PaneId::new("p\u{7}").is_err());
        assert_eq!(ModelId::new("sonnet").unwrap().as_str(), "sonnet");

        for bad in ["", "a/b", "a\\b", "..", "x..y", "r\t1"] {
            assert!(RoleName::new(bad).is_err(), "{bad:?} should be rejected");
        }
        let role = RoleName::new("opus-2").unwrap();
        assert_eq!(role.to_string(), "opus-2");

        let ws = WorkspaceId::new("w1").unwrap();
        let w = WorkerId::new_for(&ws, &role);
        assert_eq!(w.as_str(), "w1:opus-2");
        for bad in ["w1", ":r", "w1:", "w1:a:b"] {
            assert!(WorkerId::new(bad).is_err(), "{bad:?} should be rejected");
        }

        // Deserialization runs the validator; serialization is the bare string.
        assert!(serde_json::from_str::<RoleName>(r#""../etc""#).is_err());
        let back: RoleName = serde_json::from_str(r#""opus-2""#).unwrap();
        assert_eq!(back, role);
        assert_eq!(serde_json::to_string(&role).unwrap(), r#""opus-2""#);
        assert_eq!("w1".parse::<WorkspaceId>().unwrap(), ws);
        assert_eq!(TaskId::new(""), Err(IdError::Empty { kind: "TaskId" }));
        assert_eq!(
            SessionId::new("a\nb"),
            Err(IdError::ControlChar { kind: "SessionId" })
        );
        assert_eq!(
            RoleName::new("a/b"),
            Err(IdError::PathLike {
                kind: "RoleName",
                value: "a/b".into()
            })
        );
        assert_eq!(
            WorkerId::new("w1"),
            Err(IdError::BadWorkerId { value: "w1".into() })
        );
        let err = RoleName::new("a/b").unwrap_err().to_string();
        assert!(err.contains("RoleName") && err.contains("a/b"), "{err}");
    }

    #[test]
    fn arc_02_mint_v7_layout() {
        let at = DateTime::from_timestamp_millis(1_790_000_000_123).unwrap();
        let a = mint_v7(at);
        let b = mint_v7(at);
        assert_ne!(a, b, "two mints in the same millisecond must differ");
        for u in [a, b] {
            let bytes = u.as_bytes();
            assert_eq!(bytes[6] >> 4, 7, "version nibble");
            assert_eq!(bytes[8] >> 6, 0b10, "RFC 4122 variant");
            assert_eq!(u.get_version_num(), 7);
            let mut ms = [0u8; 8];
            ms[2..].copy_from_slice(&bytes[..6]);
            assert_eq!(u64::from_be_bytes(ms), 1_790_000_000_123);
        }

        let later = DateTime::from_timestamp_millis(1_790_000_000_124).unwrap();
        let c = mint_v7(later);
        assert!(a < c && b < c, "ids order by time");
        assert!(a.to_string() < c.to_string(), "and so do their strings");

        let id = ExecutionId::mint(at);
        assert_eq!(id.as_str().len(), 36);
        assert_eq!(
            uuid::Uuid::parse_str(id.as_str())
                .unwrap()
                .get_version_num(),
            7
        );
        for s in [
            EventId::mint(at).to_string(),
            ExperimentId::mint(at).to_string(),
            RoundId::mint(at).to_string(),
            JudgmentId::mint(at).to_string(),
        ] {
            assert_eq!(uuid::Uuid::parse_str(&s).unwrap().get_version_num(), 7);
        }
    }

    #[test]
    fn arc_02_legacy_ids_accepted() {
        let v4 = uuid::Uuid::new_v4().to_string();
        for legacy in ["rec-o1", "perf-3", v4.as_str()] {
            let id = ExecutionId::new(legacy).unwrap();
            assert_eq!(id.as_str(), legacy);
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(serde_json::from_str::<ExecutionId>(&json).unwrap(), id);
        }
    }
}
