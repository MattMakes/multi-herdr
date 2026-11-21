use regex_syntax::hir::{Class, Hir, HirKind};

use crate::rng::SplitMix64;

const UNBOUNDED_EXTRA: u32 = 8;
const ASCII_PREFERENCE: f64 = 0.9;

const TEXT_RANGES: &[(u32, u32)] = &[
    (0x20, 0x7E),
    (0xC0, 0xFF),
    (0x100, 0x17F),
    (0x391, 0x3C9),
    (0x410, 0x44F),
    (0x3041, 0x3096),
    (0x4E00, 0x4FFF),
    (0xAC00, 0xAD00),
    (0x1F600, 0x1F64F),
    (0x1F300, 0x1F3FF),
];

pub fn parse_regex(pattern: &str) -> Result<Hir, String> {
    regex_syntax::ParserBuilder::new()
        .build()
        .parse(&ascii_shorthands(pattern))
        .map_err(|error| format!("regex() cannot use the pattern {pattern:?}: {error}"))
}

pub fn sample_regex(pattern: &str, rng: &mut SplitMix64) -> Result<String, String> {
    let hir = parse_regex(pattern)?;
    let mut out = String::new();
    sample_hir(&hir, rng, &mut out)?;
    Ok(out)
}

fn ascii_shorthands(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str("[0-9]"),
            Some('D') => out.push_str("[^0-9]"),
            Some('w') => out.push_str("[A-Za-z0-9_]"),
            Some('W') => out.push_str("[^A-Za-z0-9_]"),
            Some('s') => out.push_str("[ \\t\\n\\r\\x0B\\x0C]"),
            Some('S') => out.push_str("[^ \\t\\n\\r\\x0B\\x0C]"),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn sample_hir(hir: &Hir, rng: &mut SplitMix64, out: &mut String) -> Result<(), String> {
    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => Ok(()),
        HirKind::Literal(literal) => {
            out.push_str(&String::from_utf8_lossy(&literal.0));
            Ok(())
        }
        HirKind::Class(class) => {
            let picked = sample_class(class, rng)
                .ok_or_else(|| "regex() met a character class that matches nothing".to_string())?;
            out.push(picked);
            Ok(())
        }
        HirKind::Repetition(repetition) => {
            let max = repetition
                .max
                .unwrap_or(repetition.min.saturating_add(UNBOUNDED_EXTRA));
            let count = repetition.min + rng.below(u64::from(max - repetition.min) + 1) as u32;
            for _ in 0..count {
                sample_hir(&repetition.sub, rng, out)?;
            }
            Ok(())
        }
        HirKind::Capture(capture) => sample_hir(&capture.sub, rng, out),
        HirKind::Concat(parts) => {
            for part in parts {
                sample_hir(part, rng, out)?;
            }
            Ok(())
        }
        HirKind::Alternation(options) => {
            let picked = rng.below(options.len() as u64) as usize;
            sample_hir(&options[picked], rng, out)
        }
    }
}

fn sample_class(class: &Class, rng: &mut SplitMix64) -> Option<char> {
    let ranges: Vec<(u32, u32)> = match class {
        Class::Unicode(unicode) => unicode
            .ranges()
            .iter()
            .map(|range| (range.start() as u32, range.end() as u32))
            .collect(),
        Class::Bytes(bytes) => bytes
            .ranges()
            .iter()
            .map(|range| (u32::from(range.start()), u32::from(range.end()).min(0x7F)))
            .filter(|(start, end)| start <= end)
            .collect(),
    };
    let printable: Vec<(u32, u32)> = ranges
        .iter()
        .filter_map(|&(start, end)| {
            let start = start.max(0x20);
            let end = end.min(0x7E);
            (start <= end).then_some((start, end))
        })
        .collect();
    if !printable.is_empty() && rng.unit_f64() < ASCII_PREFERENCE {
        return pick_from_ranges(&printable, rng);
    }
    pick_from_ranges(&ranges, rng)
}

fn pick_from_ranges(ranges: &[(u32, u32)], rng: &mut SplitMix64) -> Option<char> {
    let total: u64 = ranges
        .iter()
        .map(|&(start, end)| u64::from(end - start) + 1)
        .sum();
    if total == 0 {
        return None;
    }
    let mut ticket = rng.below(total);
    for &(start, end) in ranges {
        let size = u64::from(end - start) + 1;
        if ticket < size {
            let code = start + ticket as u32;
            return char::from_u32(code).or_else(|| char::from_u32(start));
        }
        ticket -= size;
    }
    None
}

pub fn sample_text(min: u32, max: u32, rng: &mut SplitMix64) -> String {
    let length = min + rng.below(u64::from(max.saturating_sub(min)) + 1) as u32;
    let mut out = String::new();
    for _ in 0..length {
        let range = TEXT_RANGES[rng.below(TEXT_RANGES.len() as u64) as usize];
        if let Some(picked) = pick_from_ranges(&[range], rng) {
            out.push(picked);
        }
    }
    out
}

pub fn sample_email(rng: &mut SplitMix64) -> String {
    const LOCAL: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    const LOCAL_INNER: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789._+-";
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    let mut out = String::new();
    let local_length = 1 + rng.below(12) as usize;
    for position in 0..local_length {
        let alphabet = if position == 0 || position + 1 == local_length {
            LOCAL
        } else {
            LOCAL_INNER
        };
        out.push(pick_byte(alphabet, rng));
    }
    out.push('@');
    let labels = 1 + rng.below(2);
    for _ in 0..labels {
        let length = 1 + rng.below(10);
        for _ in 0..length {
            out.push(pick_byte(LOCAL, rng));
        }
        out.push('.');
    }
    let tld_length = 2 + rng.below(5);
    for _ in 0..tld_length {
        out.push(pick_byte(LETTERS, rng));
    }
    out
}

fn pick_byte(alphabet: &[u8], rng: &mut SplitMix64) -> char {
    char::from(alphabet[rng.below(alphabet.len() as u64) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, text: &str) -> bool {
        match pattern {
            "[a-c]{3}" => text.len() == 3 && text.chars().all(|c| ('a'..='c').contains(&c)),
            "(cat|dog)s?" => ["cat", "cats", "dog", "dogs"].contains(&text),
            r"[\d.]{2,4}-x" => {
                let (digits, rest) = text.split_at(text.len() - 2);
                rest == "-x"
                    && (2..=4).contains(&digits.len())
                    && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            }
            _ => false,
        }
    }

    #[test]
    fn regex_samples_match_their_pattern() {
        let mut rng = SplitMix64::new(11);
        for pattern in ["[a-c]{3}", "(cat|dog)s?", r"[\d.]{2,4}-x"] {
            for _ in 0..200 {
                let text = sample_regex(pattern, &mut rng).unwrap();
                assert!(matches(pattern, &text), "{pattern} gave {text:?}");
            }
        }
    }

    #[test]
    fn a_bad_pattern_is_an_error() {
        assert!(sample_regex("(", &mut SplitMix64::new(1)).is_err());
    }

    #[test]
    fn text_respects_its_length_bounds_and_avoids_controls() {
        let mut rng = SplitMix64::new(5);
        for _ in 0..300 {
            let text = sample_text(2, 6, &mut rng);
            let count = text.chars().count();
            assert!((2..=6).contains(&count), "{text:?}");
            assert!(!text.chars().any(char::is_control), "{text:?}");
        }
    }

    #[test]
    fn emails_have_one_at_sign_and_a_dotted_domain() {
        let mut rng = SplitMix64::new(3);
        for _ in 0..300 {
            let address = sample_email(&mut rng);
            let (local, domain) = address.split_once('@').unwrap();
            assert!(!local.is_empty() && !local.starts_with('.') && !local.ends_with('.'));
            assert!(domain.contains('.') && !domain.contains('@'), "{address}");
            let tld = domain.rsplit('.').next().unwrap();
            assert!(tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_lowercase()));
        }
    }
}
