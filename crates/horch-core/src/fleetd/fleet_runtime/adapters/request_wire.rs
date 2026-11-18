//! The wire form of the request contract: the JSON line of a request and of
//! its reply, and `payload_sha256`. A request file holds the same JSON
//! object as a request line. The rules of the kind, the `request_id` and
//! the size are in `domain/requests.rs`.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 08 of fleetd-p5.

use std::fmt;

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::fleetd::fleet_runtime::domain::requests::{check_size, Kind, RequestError, RequestId};

/// 1 request: `{"request_id", "kind", "payload", "token"}`. The payload
/// never changes for 1 `request_id`.
#[derive(Clone, PartialEq)]
pub struct Request {
    pub request_id: RequestId,
    pub kind: Kind,
    pub payload: Map<String, Value>,
    /// The operator token, or `None` when the line has no `token`.
    pub token: Option<String>,
}

/// The token is a secret: `Debug` never prints it.
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("request_id", &self.request_id)
            .field("kind", &self.kind)
            .field("payload", &self.payload)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    request_id: String,
    kind: String,
    payload: Map<String, Value>,
    #[serde(default)]
    token: Option<String>,
}

impl Request {
    /// The request as 1 JSON line, with its `\n`.
    pub fn to_line(&self) -> String {
        let mut wire = Map::new();
        wire.insert("request_id".into(), self.request_id.as_str().into());
        wire.insert("kind".into(), self.kind.as_str().into());
        wire.insert("payload".into(), Value::Object(self.payload.clone()));
        if let Some(token) = &self.token {
            wire.insert("token".into(), token.as_str().into());
        }
        format!("{}\n", Value::Object(wire))
    }

    /// The sha256 of the payload of this request.
    pub fn payload_sha256(&self) -> String {
        payload_sha256(&self.payload)
    }
}

/// Parses 1 request line or request file of `line`. It refuses a line over
/// `max_bytes` before it parses it.
pub fn parse_request(line: &[u8], max_bytes: u64) -> Result<Request, RequestError> {
    check_size(line.len(), max_bytes)?;
    let wire: WireRequest =
        serde_json::from_slice(line).map_err(|e| RequestError::malformed(&e.to_string()))?;
    Ok(Request {
        request_id: RequestId::parse(&wire.request_id)?,
        kind: Kind::parse(&wire.kind)?,
        payload: wire.payload,
        token: wire.token,
    })
}

/// The lower-case hex sha256 of `payload` with the keys of each object
/// sorted, so that 2 equal payloads always have 1 hash.
pub fn payload_sha256(payload: &Map<String, Value>) -> String {
    let mut text = String::new();
    write_sorted_object(payload, &mut text);
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes `object` as compact JSON to `out`, with its keys sorted. The sort
/// does not depend on the key order of `Map`.
fn write_sorted_object(object: &Map<String, Value>, out: &mut String) {
    let mut entries: Vec<(&String, &Value)> = object.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    out.push('{');
    for (n, (key, value)) in entries.into_iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        out.push_str(&Value::from(key.as_str()).to_string());
        out.push(':');
        write_sorted(value, out);
    }
    out.push('}');
}

fn write_sorted(value: &Value, out: &mut String) {
    match value {
        Value::Object(object) => write_sorted_object(object, out),
        Value::Array(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_sorted(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// What fleetd did with a request.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Applied(Value),
    Refused(String),
}

/// 1 reply: `{"request_id", "state": "applied", "result"}` or
/// `{"request_id", "state": "refused", "reason"}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub request_id: RequestId,
    pub outcome: Outcome,
}

impl Reply {
    /// The reply as 1 JSON line, with its `\n`.
    pub fn to_line(&self) -> String {
        let mut wire = Map::new();
        wire.insert("request_id".into(), self.request_id.as_str().into());
        match &self.outcome {
            Outcome::Applied(result) => {
                wire.insert("state".into(), "applied".into());
                wire.insert("result".into(), result.clone());
            }
            Outcome::Refused(reason) => {
                wire.insert("state".into(), "refused".into());
                wire.insert("reason".into(), reason.as_str().into());
            }
        }
        format!("{}\n", Value::Object(wire))
    }
}

/// Parses 1 reply line of `line`. It refuses a line over `max_bytes`
/// before it parses it.
pub fn parse_reply(line: &[u8], max_bytes: u64) -> Result<Reply, RequestError> {
    check_size(line.len(), max_bytes)?;
    let mut wire: Map<String, Value> =
        serde_json::from_slice(line).map_err(|e| RequestError::malformed(&e.to_string()))?;
    let request_id = match wire.remove("request_id") {
        Some(Value::String(id)) => RequestId::parse(&id)?,
        _ => return Err(RequestError::malformed("reply has no text request_id")),
    };
    let outcome = match (
        wire.remove("state"),
        wire.remove("result"),
        wire.remove("reason"),
    ) {
        (Some(state), Some(result), None) if state == "applied" => Outcome::Applied(result),
        (Some(state), None, Some(Value::String(reason))) if state == "refused" => {
            Outcome::Refused(reason)
        }
        _ => {
            return Err(RequestError::malformed(
                "reply is not state applied with result, or state refused with a text reason",
            ))
        }
    };
    if let Some(key) = wire.keys().next() {
        return Err(RequestError::malformed(&format!(
            "reply has the unknown field `{key}`"
        )));
    }
    Ok(Reply {
        request_id,
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MAX: u64 = 65_536;
    const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";

    fn object(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn equal_payloads_with_other_key_order_have_1_hash() {
        let a = br#"{"b": 1, "a": {"y": [1, {"q": 2, "p": 3}], "x": "s"}}"#;
        let b = br#"{"a": {"x": "s", "y": [1, {"p": 3, "q": 2}]}, "b": 1}"#;
        let a: Map<String, Value> = serde_json::from_slice(a).unwrap();
        let b: Map<String, Value> = serde_json::from_slice(b).unwrap();
        let hash = payload_sha256(&a);
        assert_eq!(hash, payload_sha256(&b));
        assert_eq!(hash.len(), 64);
        assert!(hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        let other = object(json!({"a": {"x": "s", "y": [{"p": 3, "q": 2}, 1]}, "b": 1}));
        assert_ne!(hash, payload_sha256(&other), "array order counts");
        // The canonical text is `{}`: sha256 of the 2 bytes `{}`.
        assert_eq!(
            payload_sha256(&Map::new()),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }

    #[test]
    fn line_over_the_limit_is_refused_before_parse() {
        let garbage = vec![b'{'; 65];
        assert_eq!(
            parse_request(&garbage, 64),
            Err(RequestError::TooLarge {
                bytes: 65,
                max_bytes: 64
            })
        );
        assert_eq!(
            parse_reply(&garbage, 64),
            Err(RequestError::TooLarge {
                bytes: 65,
                max_bytes: 64
            })
        );
        assert!(matches!(
            parse_request(&garbage[..64], 64),
            Err(RequestError::Malformed(_))
        ));
    }

    #[test]
    fn request_round_trips_and_refuses_each_bad_field() {
        let req = Request {
            request_id: RequestId::parse(ID).unwrap(),
            kind: Kind::TaskCancel,
            payload: object(json!({"task": "T-1"})),
            token: Some("secret".into()),
        };
        let line = req.to_line();
        assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
        assert_eq!(parse_request(line.as_bytes(), MAX), Ok(req.clone()));
        assert!(
            !format!("{req:?}").contains("secret"),
            "Debug hides the token"
        );
        let no_token = Request { token: None, ..req };
        assert!(!no_token.to_line().contains("token"));
        assert_eq!(
            parse_request(no_token.to_line().as_bytes(), MAX),
            Ok(no_token)
        );

        let bad = |text: &str| parse_request(text.as_bytes(), MAX).unwrap_err();
        let ok = format!(r#""request_id":"{ID}","kind":"spawn","payload":{{}}"#);
        assert!(parse_request(format!("{{{ok}}}").as_bytes(), MAX).is_ok());
        assert!(matches!(
            bad(&format!(r#"{{{ok},"extra":1}}"#)),
            RequestError::Malformed(_)
        ));
        assert!(matches!(
            bad(r#"{"request_id":"a/b","kind":"spawn","payload":{}}"#),
            RequestError::BadRequestId(_)
        ));
        assert_eq!(
            bad(&format!(
                r#"{{"request_id":"{ID}","kind":"idea_show","payload":{{}}}}"#
            )),
            RequestError::UnknownKind("idea_show".into())
        );
        for text in [
            format!(r#"{{"request_id":"{ID}","kind":"spawn","payload":[]}}"#),
            format!(r#"{{"request_id":"{ID}","kind":"spawn"}}"#),
            format!(r#"{{"request_id":"{ID}","kind":"spawn","payload":{{}},"token":1}}"#),
            format!(r#"{{{ok},"kind":"spawn"}}"#),
            format!(r#"{{{ok}}} {{}}"#),
            "\u{1b}[2J".to_string(),
        ] {
            let err = bad(&text);
            assert!(matches!(err, RequestError::Malformed(_)), "{text}: {err}");
            assert!(!err.to_string().chars().any(char::is_control), "{err:?}");
        }
    }

    #[test]
    fn reply_round_trips() {
        let id = RequestId::parse(ID).unwrap();
        let applied = Reply {
            request_id: id.clone(),
            outcome: Outcome::Applied(json!({"task": "T-1", "state": "cancelled"})),
        };
        let refused = Reply {
            request_id: id.clone(),
            outcome: Outcome::Refused("no valid operator token".into()),
        };
        let null_result = Reply {
            request_id: id,
            outcome: Outcome::Applied(Value::Null),
        };
        for reply in [applied, refused, null_result] {
            let line = reply.to_line();
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            assert_eq!(parse_reply(line.as_bytes(), MAX), Ok(reply));
        }
        let v: Value = serde_json::from_str(
            &Reply {
                request_id: RequestId::parse(ID).unwrap(),
                outcome: Outcome::Refused("r".into()),
            }
            .to_line(),
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"request_id": ID, "state": "refused", "reason": "r"})
        );

        for text in [
            format!(r#"{{"request_id":"{ID}","state":"applied"}}"#),
            format!(r#"{{"request_id":"{ID}","state":"refused"}}"#),
            format!(r#"{{"request_id":"{ID}","state":"refused","reason":1}}"#),
            format!(r#"{{"request_id":"{ID}","state":"applied","result":1,"reason":"r"}}"#),
            format!(r#"{{"request_id":"{ID}","state":"pending","result":1}}"#),
            format!(r#"{{"request_id":"{ID}","state":"applied","result":1,"x":1}}"#),
            r#"{"request_id":"a/b","state":"applied","result":1}"#.to_string(),
            r#"[]"#.to_string(),
        ] {
            assert!(parse_reply(text.as_bytes(), MAX).is_err(), "{text}");
        }
    }
}
