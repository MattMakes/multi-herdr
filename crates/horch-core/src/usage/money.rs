//! Exact integer money (MEA-07): whole micro-dollars ([`MicroUsd`]) and
//! where a cost figure came from ([`CostSource`]).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Whole micro-dollars (1e-6 USD), the stored unit.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MicroUsd(pub i64);

/// Where a cost figure came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CostSource {
    /// The compiled-in price table, as of `date`.
    PriceTable { date: String },
    /// A `--pricing` file, named by the first 12 hex chars of its sha256.
    PricingFile { sha12: String },
    /// The harness reported the cost itself.
    HarnessReported,
    /// No price was known; the cost is not counted.
    Unpriced,
}

impl fmt::Display for CostSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostSource::PriceTable { date } => write!(f, "price_table@{date}"),
            CostSource::PricingFile { sha12 } => write!(f, "pricing_file@{sha12}"),
            CostSource::HarnessReported => f.write_str("harness_reported"),
            CostSource::Unpriced => f.write_str("unpriced"),
        }
    }
}

/// Why a string is not a [`CostSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCostSourceError(String);

impl fmt::Display for ParseCostSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is not a cost source (price_table@<date>, pricing_file@<sha12>, \
             harness_reported or unpriced)",
            self.0
        )
    }
}

impl std::error::Error for ParseCostSourceError {}

impl FromStr for CostSource {
    type Err = ParseCostSourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "harness_reported" => Ok(CostSource::HarnessReported),
            "unpriced" => Ok(CostSource::Unpriced),
            _ => parse_dated(s),
        }
    }
}

/// The `price_table@<date>` and `pricing_file@<sha12>` forms.
fn parse_dated(s: &str) -> Result<CostSource, ParseCostSourceError> {
    match s.split_once('@') {
        Some(("price_table", date)) if !date.is_empty() => {
            Ok(CostSource::PriceTable { date: date.into() })
        }
        Some(("pricing_file", sha12)) if is_sha12(sha12) => Ok(CostSource::PricingFile {
            sha12: sha12.into(),
        }),
        _ => Err(ParseCostSourceError(s.to_string())),
    }
}

fn is_sha12(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

impl Serialize for CostSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CostSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text that is not a cost source is refused with its own text in
    /// the error: unknown names, an empty date, and a hash that is not 12
    /// lower-case hex digits.
    #[test]
    fn mea_07_cost_source_refuses_malformed_text() {
        for bad in [
            "",
            "free",
            "price_table@",
            "price_table",
            "pricing_file@",
            "pricing_file@ba7816bf8f0",
            "pricing_file@ba7816bf8f011",
            "pricing_file@BA7816BF8F01",
            "pricing_file@ba7816bf8f0g",
            "harness_reported ",
        ] {
            let err = bad.parse::<CostSource>().unwrap_err();
            assert_eq!(err, ParseCostSourceError(bad.to_string()), "{bad:?}");
        }
    }

    #[test]
    fn mea_07_cost_source() {
        let cases = [
            (
                CostSource::PriceTable {
                    date: "2026-09-24".into(),
                },
                "price_table@2026-09-24",
            ),
            (
                CostSource::PricingFile {
                    sha12: "ba7816bf8f01".into(),
                },
                "pricing_file@ba7816bf8f01",
            ),
            (CostSource::HarnessReported, "harness_reported"),
            (CostSource::Unpriced, "unpriced"),
        ];
        for (src, text) in cases {
            assert_eq!(src.to_string(), text);
            assert_eq!(text.parse::<CostSource>().unwrap(), src);
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(json, format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<CostSource>(&json).unwrap(), src);
        }
        for bad in [
            "",
            "price_table@",
            "pricing_file@xyz",
            "pricing_file@BA7816BF8F01",
            "Unpriced",
        ] {
            assert!(bad.parse::<CostSource>().is_err(), "{bad}");
        }
        assert_eq!(serde_json::to_string(&MicroUsd(42)).unwrap(), "42");
    }
}
