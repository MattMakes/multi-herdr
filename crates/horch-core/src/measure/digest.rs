//! sha256 digests over canonical JSON.
//!
//! A digest names a task, a config, a judge policy or an environment, so the
//! same value must give the same digest on every machine and every run. The
//! canonical form sorts object keys recursively and drops insignificant
//! whitespace; numbers and strings are printed exactly as serde_json prints
//! them.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

/// A sha256 digest, written as `sha256:<64 lowercase hex>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(pub [u8; 32]);

const PREFIX: &str = "sha256:";

impl Digest {
    /// The first 12 hex characters, for file names and labels.
    pub fn short12(&self) -> String {
        self.hex()[..12].to_string()
    }

    /// The 64 hex characters without the `sha256:` prefix.
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.hex())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

/// Why a string is not a [`Digest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDigestError(String);

impl fmt::Display for ParseDigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is not a digest (expected sha256:<64 lowercase hex>)",
            self.0
        )
    }
}

impl std::error::Error for ParseDigestError {}

impl FromStr for Digest {
    type Err = ParseDigestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseDigestError(s.to_string());
        let hex = s.strip_prefix(PREFIX).ok_or_else(err)?;
        if hex.len() != 64 {
            return Err(err());
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        let mut out = [0u8; 32];
        for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
            out[i] = (nibble(pair[0]).ok_or_else(err)? << 4) | nibble(pair[1]).ok_or_else(err)?;
        }
        Ok(Digest(out))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// sha256 of raw bytes.
pub fn sha256_bytes(bytes: &[u8]) -> Digest {
    Digest(Sha256::digest(bytes).into())
}

/// The canonical text of a JSON value: keys sorted at every depth, no
/// whitespace between tokens.
pub fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // Sorted here, not by the map: serde_json's `preserve_order`
            // feature can be switched on by any crate in the build.
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_canonical(v, out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).expect("a string serializes"));
}

/// sha256 of the canonical JSON of `value`.
pub fn digest_json<T: Serialize + ?Sized>(value: &T) -> Digest {
    let v = serde_json::to_value(value).expect("a digested value serializes to JSON");
    sha256_bytes(canonical_json(&v).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mea_01_digests_stable() {
        assert_eq!(
            sha256_bytes(b"").to_string(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc").to_string(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let a = json!({"c": {"z": -2.5, "y": "é\n"}, "b": [true, null, "x"], "a": 1});
        assert_eq!(
            canonical_json(&a),
            "{\"a\":1,\"b\":[true,null,\"x\"],\"c\":{\"y\":\"é\\n\",\"z\":-2.5}}"
        );
        let want = "sha256:fef91f37d67e4bb1c37368baf19dd640ab0703913f900546b4792ec580e3906d";
        assert_eq!(digest_json(&a).to_string(), want);

        // Key order and whitespace in the source text do not matter.
        let b: serde_json::Value = serde_json::from_str(
            "{ \"a\" : 1,\n  \"c\": { \"y\": \"é\\n\", \"z\": -2.5 },\n  \"b\": [ true, null, \"x\" ] }",
        )
        .unwrap();
        assert_eq!(digest_json(&b).to_string(), want);

        // A struct digests the same as its JSON value.
        #[derive(Serialize)]
        struct S {
            b: u32,
            a: &'static str,
        }
        assert_eq!(
            digest_json(&S { b: 2, a: "x" }),
            digest_json(&json!({"a": "x", "b": 2}))
        );
    }

    #[test]
    fn digest_string_round_trips() {
        let d = sha256_bytes(b"abc");
        assert_eq!(d.short12(), "ba7816bf8f01");
        assert_eq!(d.to_string().parse::<Digest>().unwrap(), d);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, format!("\"{d}\""));
        assert_eq!(serde_json::from_str::<Digest>(&json).unwrap(), d);
        for bad in [
            "",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
            "sha256:ba78",
            "sha256:zz7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ] {
            assert!(bad.parse::<Digest>().is_err(), "{bad}");
        }
    }
}
