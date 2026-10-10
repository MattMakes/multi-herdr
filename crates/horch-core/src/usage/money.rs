//! Exact integer money (MEA-07).
//!
//! Dataset rows store cost as whole micro-dollars ([`MicroUsd`]). Sums are
//! taken in nano-dollars (`NanoUsd`), where token count × price is exact:
//! a price in $/MTok × 1000 is n$/token, a whole number for every built-in
//! price. The total is rounded to µ$ once, half-even, so the order in which
//! terms are added never changes the stored cost.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Whole micro-dollars (1e-6 USD), the stored unit.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MicroUsd(pub i64);

/// Whole nano-dollars (1e-9 USD), the accumulation unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NanoUsd(pub i128);

/// What can go wrong converting a price.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MoneyError {
    /// The $/MTok price is not a whole, non-negative number of n$/token.
    InexactPrice { dollars_per_mtok: f64 },
    /// The price does not fit the integer range.
    Overflow,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyError::InexactPrice { dollars_per_mtok } => write!(
                f,
                "price ${dollars_per_mtok}/MTok is not a whole, non-negative number of \
                 nano-dollars per token"
            ),
            MoneyError::Overflow => f.write_str("price is too large"),
        }
    }
}

impl std::error::Error for MoneyError {}

/// How far from a whole n$ a scaled price may be and still count as exact.
const EXACT_EPSILON: f64 = 1e-9;

/// Nano-dollars per token for a price in dollars per million tokens.
pub(crate) fn nano_per_token(dollars_per_mtok: f64) -> Result<i128, MoneyError> {
    let inexact = MoneyError::InexactPrice { dollars_per_mtok };
    if dollars_per_mtok.is_nan() || dollars_per_mtok < 0.0 {
        return Err(inexact);
    }
    let scaled = dollars_per_mtok * 1000.0;
    if scaled >= i64::MAX as f64 {
        return Err(MoneyError::Overflow);
    }
    let whole = scaled.round();
    if (scaled - whole).abs() > EXACT_EPSILON {
        return Err(inexact);
    }
    Ok(whole as i128)
}

impl NanoUsd {
    /// The cost of `tokens` tokens at `nano_per_token` n$ each.
    pub(crate) fn of(tokens: u64, nano_per_token: i128) -> Self {
        NanoUsd(tokens as i128 * nano_per_token)
    }

    /// Add the cost of `tokens` tokens at `nano_per_token` n$ each.
    pub(crate) fn add_tokens(&mut self, tokens: u64, nano_per_token: i128) {
        *self += NanoUsd::of(tokens, nano_per_token);
    }

    /// Round to whole µ$, half to even. Saturates at the `i64` range.
    pub(crate) fn to_micro_half_even(self) -> MicroUsd {
        let q = self.0.div_euclid(1000);
        let r = self.0.rem_euclid(1000);
        let q = if r > 500 || (r == 500 && q % 2 != 0) {
            q + 1
        } else {
            q
        };
        MicroUsd(q.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
    }
}

impl std::ops::Add for NanoUsd {
    type Output = NanoUsd;
    fn add(self, rhs: NanoUsd) -> NanoUsd {
        NanoUsd(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for NanoUsd {
    fn add_assign(&mut self, rhs: NanoUsd) {
        self.0 += rhs.0;
    }
}

impl std::iter::Sum for NanoUsd {
    fn sum<I: Iterator<Item = NanoUsd>>(iter: I) -> NanoUsd {
        iter.fold(NanoUsd(0), |a, b| a + b)
    }
}

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
        let err = || ParseCostSourceError(s.to_string());
        match s {
            "harness_reported" => return Ok(CostSource::HarnessReported),
            "unpriced" => return Ok(CostSource::Unpriced),
            _ => {}
        }
        if let Some(date) = s.strip_prefix("price_table@").filter(|d| !d.is_empty()) {
            return Ok(CostSource::PriceTable { date: date.into() });
        }
        let sha12 = s.strip_prefix("pricing_file@").ok_or_else(err)?;
        if sha12.len() == 12
            && sha12
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        {
            Ok(CostSource::PricingFile {
                sha12: sha12.into(),
            })
        } else {
            Err(err())
        }
    }
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

    #[test]
    fn mea_07_every_builtin_price_is_exact() {
        for (model, p) in super::super::builtin_prices() {
            // A cache write is the listed price, or the derived multiple of
            // input when none is listed (`Price::cost`, `budget.rs`).
            let all = [
                p.input,
                p.output,
                p.cache_read,
                p.cache_write_5m.unwrap_or(p.input * 1.25),
                p.cache_write_1h.unwrap_or(p.input * 2.0),
            ];
            for price in all {
                nano_per_token(price).unwrap_or_else(|e| panic!("{model}: {e}"));
            }
        }
        assert_eq!(nano_per_token(4.0), Ok(4_000));
        assert_eq!(nano_per_token(0.02), Ok(20));
        assert_eq!(
            nano_per_token(0.0001),
            Err(MoneyError::InexactPrice {
                dollars_per_mtok: 0.0001
            })
        );
        assert!(matches!(
            nano_per_token(-1.0),
            Err(MoneyError::InexactPrice { .. })
        ));
        assert!(matches!(
            nano_per_token(f64::NAN),
            Err(MoneyError::InexactPrice { .. })
        ));
        assert_eq!(nano_per_token(f64::INFINITY), Err(MoneyError::Overflow));
        assert_eq!(nano_per_token(1e20), Err(MoneyError::Overflow));
    }

    #[test]
    fn mea_07_nano_accumulation_exact() {
        // gpt-5.6-luna cache reads: $0.02/MTok, 20 n$/token. In f64 dollars,
        // a million single-token additions drift; in n$ they do not.
        let per = nano_per_token(0.02).unwrap();
        let mut total = NanoUsd(0);
        let mut f = 0.0f64;
        for _ in 0..1_000_000 {
            total.add_tokens(1, per);
            f += 0.02 / 1_000_000.0;
        }
        assert_eq!(total, NanoUsd(20_000_000));
        assert_eq!(total.to_micro_half_even(), MicroUsd(20_000));
        assert_ne!(f, 0.02, "the f64 sum drifts, which is why money is integer");

        let parts = [
            NanoUsd::of(123_456, nano_per_token(4.0).unwrap()),
            NanoUsd::of(7_890, nano_per_token(20.0).unwrap()),
            NanoUsd::of(1_000_000, nano_per_token(0.20).unwrap()),
        ];
        let sum: NanoUsd = parts.iter().copied().sum();
        assert_eq!(sum, NanoUsd(493_824_000 + 157_800_000 + 200_000_000));
    }

    #[test]
    fn mea_07_rounding_once() {
        let micro = |n| NanoUsd(n).to_micro_half_even();
        assert_eq!(micro(1500), MicroUsd(2));
        assert_eq!(micro(2500), MicroUsd(2));
        assert_eq!(micro(3500), MicroUsd(4));
        assert_eq!(micro(2501), MicroUsd(3));
        assert_eq!(micro(2499), MicroUsd(2));
        assert_eq!(micro(0), MicroUsd(0));
        assert_eq!(micro(-1500), MicroUsd(-2));
        assert_eq!(micro(-2500), MicroUsd(-2));
        assert_eq!(micro(-2501), MicroUsd(-3));

        // Sum then round once, not round each term.
        let terms = [NanoUsd(1500), NanoUsd(1500)];
        let once = terms.iter().copied().sum::<NanoUsd>().to_micro_half_even();
        let each: i64 = terms.iter().map(|t| t.to_micro_half_even().0).sum();
        assert_eq!(once, MicroUsd(3));
        assert_eq!(each, 4);
        assert_ne!(once.0, each);
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
