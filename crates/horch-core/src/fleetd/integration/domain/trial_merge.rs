//! A trial merge, keyed by its 2 tips: a feature against the primary
//! branch, or 2 features that share a file.
//!
//! `docs/specs/fleetd/design.md`, "Between features", "Early detection":
//! each in-flight feature against the primary branch, and 1 trial merge for
//! each pair of in-flight features whose sets of changed files share a
//! file. A pair with no shared file needs no trial merge. The rules here
//! have no I/O.

use std::collections::BTreeSet;

/// 1 side of a trial merge: the primary branch or a feature, its ref and
/// its tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    /// The name in the subject of the job: the primary branch name, or the
    /// feature id.
    pub name: String,
    /// The full ref name, for example `refs/heads/fleet/F-0001`.
    pub ref_name: String,
    pub tip: String,
}

/// An in-flight feature (`open` or `ready`): its side and the files that
/// its branch changed from its merge base with the primary branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub side: Side,
    pub files: BTreeSet<String>,
}

/// A trial merge to do: `left` is the primary branch or the older feature,
/// `right` the feature or the younger feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub left: Side,
    pub right: Side,
}

impl Pair {
    /// The subject of its job: the 2 sides, for example `main+F-0001` or
    /// `F-0001+F-0002`.
    pub fn subject(&self) -> String {
        format!("{}+{}", self.left.name, self.right.name)
    }

    /// The input key of its job: the 2 tips (The kinds of job).
    pub fn input_key(&self) -> String {
        input_key(&self.left.tip, &self.right.tip)
    }
}

/// The input key of a trial merge of `left_tip` into `right_tip`.
pub fn input_key(left_tip: &str, right_tip: &str) -> String {
    format!("{left_tip} {right_tip}")
}

/// The 2 tips of the input key `key`, or `None` when it is not 2 tips.
pub fn tips(key: &str) -> Option<(&str, &str)> {
    let (left, right) = key.split_once(' ')?;
    let one_word = |t: &str| !t.is_empty() && !t.contains(' ');
    (one_word(left) && one_word(right)).then_some((left, right))
}

/// The outcome of a trial merge. Clean and conflict are both verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    /// The files that conflict, sorted, each 1 time.
    Conflict(Vec<String>),
}

/// Each trial merge that the in-flight `features` need against `primary`,
/// in order: each feature against the primary branch, then each pair of
/// features that share a changed file. `features` is oldest first, so the
/// left side of a pair is the older feature.
pub fn pairs<'a>(primary: &'a Side, features: &'a [InFlight]) -> impl Iterator<Item = Pair> + 'a {
    let against_primary = features.iter().map(move |f| Pair {
        left: primary.clone(),
        right: f.side.clone(),
    });
    let between = features.iter().enumerate().flat_map(move |(i, older)| {
        features[i + 1..]
            .iter()
            .filter(move |younger| !older.files.is_disjoint(&younger.files))
            .map(move |younger| Pair {
                left: older.side.clone(),
                right: younger.side.clone(),
            })
    });
    against_primary.chain(between)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(name: &str, tip: &str) -> Side {
        Side {
            name: name.into(),
            ref_name: format!("refs/heads/{name}"),
            tip: tip.into(),
        }
    }

    fn feature(name: &str, files: &[&str]) -> InFlight {
        InFlight {
            side: side(name, &format!("{name}-tip")),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn pairs_are_each_feature_on_primary_then_each_sharing_pair() {
        let main = side("main", "m");
        let fs = [
            feature("F-1", &["a"]),
            feature("F-2", &["b"]),
            feature("F-3", &["a", "b"]),
        ];
        let subjects: Vec<String> = pairs(&main, &fs).map(|p| p.subject()).collect();
        assert_eq!(
            subjects,
            ["main+F-1", "main+F-2", "main+F-3", "F-1+F-3", "F-2+F-3"]
        );
    }

    #[test]
    fn input_key_round_trips_its_2_tips() {
        let key = input_key("aaa", "bbb");
        assert_eq!(tips(&key), Some(("aaa", "bbb")));
        assert_eq!(tips("aaa"), None);
        assert_eq!(tips("aaa bbb ccc"), None);
        assert_eq!(tips(" bbb"), None);
    }
}
