use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    Empty { kind: &'static str },
    ControlChar { kind: &'static str },
    PathLike { kind: &'static str, value: String },
    BadWorkerId { value: String },
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

fn validate_plain(kind: &'static str, value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        Err(IdError::Empty { kind })
    } else if value.chars().any(char::is_control) {
        Err(IdError::ControlChar { kind })
    } else {
        Ok(())
    }
}

fn validate_execution_id(kind: &'static str, value: &str) -> Result<(), IdError> {
    validate_plain(kind, value)
}

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

string_id!(ExecutionId, validate_execution_id);
string_id!(TaskId, validate_plain);
string_id!(SessionId, validate_plain);
string_id!(WorkerId, validate_worker_id);
string_id!(RoleName, validate_role_name);
string_id!(SkillId, validate_plain);
string_id!(ModelId, validate_plain);
string_id!(TeammateName, validate_plain);
string_id!(PaneId, validate_plain);
string_id!(WorkspaceId, validate_plain);
string_id!(ExperimentId, validate_plain);
string_id!(RoundId, validate_plain);
string_id!(EventId, validate_plain);
string_id!(JudgmentId, validate_plain);

impl WorkerId {
    pub(crate) fn new_for(workspace: &WorkspaceId, role: &RoleName) -> Self {
        Self(format!("{workspace}:{role}"))
    }
}

impl ExperimentId {
    pub fn short(&self) -> String {
        let plain: Vec<char> = self.0.chars().filter(char::is_ascii_alphanumeric).collect();
        plain[plain.len().saturating_sub(8)..].iter().collect()
    }
}

macro_rules! mintable {
    ($($name:ident),*) => {$(
        impl $name {
            pub fn mint(at: DateTime<Utc>) -> Self {
                Self(mint_v7(at).to_string())
            }
        }
    )*};
}

mintable!(ExecutionId, EventId, ExperimentId, RoundId, JudgmentId);

pub(crate) fn mint_v7(at: DateTime<Utc>) -> uuid::Uuid {
    let mut bytes = *uuid::Uuid::new_v4().as_bytes();
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
    fn pre_01_experiment_short_is_the_random_tail() {
        let at = DateTime::from_timestamp_millis(1_790_000_000_123).unwrap();
        let a = ExperimentId::mint(at);
        let b = ExperimentId::mint(at);
        assert_eq!(a.short().len(), 8);
        assert!(a.short().bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(a.as_str().ends_with(&a.short()));
        assert_ne!(
            a.short(),
            b.short(),
            "the same millisecond, different tails"
        );
        assert_eq!(ExperimentId::new("exp-1").unwrap().short(), "exp1");
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
