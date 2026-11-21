use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub name: String,
    pub ref_name: String,
    pub tip: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub side: Side,
    pub files: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub left: Side,
    pub right: Side,
}

impl Pair {
    pub fn subject(&self) -> String {
        format!("{}+{}", self.left.name, self.right.name)
    }

    pub fn input_key(&self) -> String {
        input_key(&self.left.tip, &self.right.tip)
    }
}

pub fn input_key(left_tip: &str, right_tip: &str) -> String {
    format!("{left_tip} {right_tip}")
}

pub fn tips(key: &str) -> Option<(&str, &str)> {
    let (left, right) = key.split_once(' ')?;
    let one_word = |t: &str| !t.is_empty() && !t.contains(' ');
    (one_word(left) && one_word(right)).then_some((left, right))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Conflict(Vec<String>),
}

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
