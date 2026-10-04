//! Secret redaction (SEC-01). Prompts, outputs, env metadata and diffs pass
//! through [`redact`] before they are persisted.
//!
//! The matchers are hand-written; there is no regex crate in the build. A
//! token-shaped secret (`sk-…`, `ghp_…`, `AKIA…`, a PEM private key) is
//! replaced whole. For `KEY=value` shapes the key and separator stay, so a
//! reader can still see which setting was there, and only the value goes.

use std::borrow::Cow;

/// What a secret is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// `text` with every recognized secret replaced by [`REDACTED`]. Borrows
/// when nothing matched.
pub fn redact(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        // Every pattern starts with an ASCII byte, which is always a char
        // boundary.
        if bytes[i].is_ascii() {
            if let Some((keep, end)) = match_at(text, i) {
                let o = out.get_or_insert_with(|| String::with_capacity(text.len()));
                o.push_str(&text[copied..keep]);
                o.push_str(REDACTED);
                copied = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    match out {
        None => Cow::Borrowed(text),
        Some(mut o) => {
            o.push_str(&text[copied..]);
            Cow::Owned(o)
        }
    }
}

/// An environment value as it may be recorded: fully redacted when the key
/// names a secret, else passed through [`redact`].
pub(crate) fn redact_env_value<'a>(key: &str, value: &'a str) -> Cow<'a, str> {
    let k = key.to_ascii_uppercase();
    let secret_key = ["KEY", "SECRET", "TOKEN", "PASSWORD", "PASSWD", "CREDENTIAL"]
        .iter()
        .any(|w| k.contains(w));
    if secret_key && !value.is_empty() {
        Cow::Borrowed(REDACTED)
    } else {
        redact(value)
    }
}

/// A match at byte `i`: text before `keep` stays, `keep..end` is replaced.
fn match_at(text: &str, i: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    pem(b, i)
        .or_else(|| assignment(b, i))
        .or_else(|| prefixed_tokens(b, i))
        .or_else(|| aws_key(b, i))
}

fn is_token_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// Whether a token may start at `i`: not in the middle of a word.
fn at_word_start(b: &[u8], i: usize) -> bool {
    i == 0 || !is_token_byte(b[i - 1])
}

fn starts_with_ci(b: &[u8], i: usize, pat: &str) -> bool {
    b.len() >= i + pat.len() && b[i..i + pat.len()].eq_ignore_ascii_case(pat.as_bytes())
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    b.get(from..)?
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| from + p)
}

/// End of a run of bytes from `i` that satisfy `ok`.
fn run_end(b: &[u8], i: usize, ok: impl Fn(u8) -> bool) -> usize {
    i + b[i..].iter().take_while(|&&c| ok(c)).count()
}

/// `-----BEGIN … PRIVATE KEY-----` through the next `-----END …-----`. An
/// unterminated block is redacted to the end of the text.
fn pem(b: &[u8], i: usize) -> Option<(usize, usize)> {
    const BEGIN: &[u8] = b"-----BEGIN ";
    if !b[i..].starts_with(BEGIN) {
        return None;
    }
    let header_end = find(b, i + BEGIN.len(), b"-----")?;
    let label = &b[i + BEGIN.len()..header_end];
    let private = label.windows(11).any(|w| w == b"PRIVATE KEY");
    if !private || label.contains(&b'\n') {
        return None;
    }
    let end = find(b, header_end + 5, b"-----END ")
        .and_then(|e| find(b, e + 9, b"-----"))
        .map_or(b.len(), |e| e + 5);
    Some((i, end))
}

/// `ANTHROPIC_API_KEY=<value>` and `(api_key|secret|token|password)` then
/// optional spaces, `:` or `=`, and the value up to whitespace or a quote.
/// The key and the value may be quoted, as in JSON or YAML.
fn assignment(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let key_len = [
        "ANTHROPIC_API_KEY",
        "api_key",
        "secret",
        "token",
        "password",
    ]
    .iter()
    .find(|k| starts_with_ci(b, i, k))?
    .len();
    let mut j = i + key_len;
    if matches!(b.get(j), Some(b'"' | b'\'')) {
        j += 1;
    }
    j = run_end(b, j, |c| c == b' ' || c == b'\t');
    if !matches!(b.get(j), Some(b':' | b'=')) {
        return None;
    }
    j = run_end(b, j + 1, |c| c == b' ' || c == b'\t');
    // A quoted value runs to its closing quote, so a password with a space
    // in it goes whole; a bare one stops at whitespace or a quote.
    let end = match b.get(j) {
        Some(&q @ (b'"' | b'\'')) => {
            j += 1;
            quoted_end(b, j, q)
        }
        _ => run_end(b, j, |c| {
            !c.is_ascii_whitespace() && c != b'"' && c != b'\''
        }),
    };
    (end > j).then_some((j, end))
}

/// End of a quoted value starting at `i`: the closing `q`, a newline or the
/// end of the text. A backslash escapes the next byte, so `\"` does not end
/// it early.
fn quoted_end(b: &[u8], mut i: usize, q: u8) -> usize {
    while i < b.len() && b[i] != q && b[i] != b'\n' {
        i += if b[i] == b'\\' && i + 1 < b.len() {
            2
        } else {
            1
        };
    }
    i.min(b.len())
}

/// `sk-ant-…`, `sk-…`, `ghp_…`, `github_pat_…` and Slack `xox?-…` tokens.
fn prefixed_tokens(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if !at_word_start(b, i) {
        return None;
    }
    // (prefix, minimum body length, body byte class)
    let slack: fn(u8) -> bool = |c| c.is_ascii_alphanumeric() || c == b'-';
    let rules: [(&str, usize, fn(u8) -> bool); 9] = [
        ("sk-ant-", 8, is_token_byte),
        ("sk-", 20, is_token_byte),
        ("ghp_", 20, |c| c.is_ascii_alphanumeric()),
        ("github_pat_", 20, |c| {
            c.is_ascii_alphanumeric() || c == b'_'
        }),
        ("xoxb-", 10, slack),
        ("xoxp-", 10, slack),
        ("xoxa-", 10, slack),
        ("xoxr-", 10, slack),
        ("xoxs-", 10, slack),
    ];
    for (prefix, min, ok) in rules {
        if b[i..].starts_with(prefix.as_bytes()) {
            let body = i + prefix.len();
            let end = run_end(b, body, ok);
            if end - body >= min {
                return Some((i, end));
            }
        }
    }
    None
}

/// `AKIA` plus exactly 16 uppercase alphanumerics (an AWS access key id).
fn aws_key(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if !at_word_start(b, i) || !b[i..].starts_with(b"AKIA") {
        return None;
    }
    let body = i + 4;
    let end = run_end(b, body, |c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let whole_word = b.get(end).is_none_or(|&c| !is_token_byte(c));
    (end - body == 16 && whole_word).then_some((i, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red(s: &str) -> String {
        redact(s).into_owned()
    }

    #[test]
    fn sec_01_redaction_patterns() {
        let body20 = "abcdefghijKLMNOPQRST0123";
        let cases: &[(&str, &str)] = &[
            // ANTHROPIC_API_KEY=
            (
                "ANTHROPIC_API_KEY=abc123 next",
                "ANTHROPIC_API_KEY=[REDACTED] next",
            ),
            // sk-ant-
            ("key sk-ant-api03-AbC_dEf-123 end", "key [REDACTED] end"),
            // sk- with at least 20 body chars
            (&format!("use sk-{body20}."), "use [REDACTED]."),
            // ghp_
            (&format!("ghp_{body20}"), "[REDACTED]"),
            // github_pat_
            (&format!("x github_pat_11AB_{body20}"), "x [REDACTED]"),
            // Slack
            ("xoxb-1234567890-abcdef", "[REDACTED]"),
            ("xoxp-1234567890-abcdef", "[REDACTED]"),
            ("xoxa-1234567890-abcdef", "[REDACTED]"),
            ("xoxr-1234567890-abcdef", "[REDACTED]"),
            ("xoxs-1234567890-abcdef", "[REDACTED]"),
            // AWS
            ("id AKIAIOSFODNN7EXAMPLE ok", "id [REDACTED] ok"),
            // PEM
            (
                "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\nabc\n-----END RSA PRIVATE KEY-----\nb",
                "a\n[REDACTED]\nb",
            ),
            ("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl", "[REDACTED]"),
            // key: value / key=value, case-insensitive, quoted
            ("api_key: hunter2 rest", "api_key: [REDACTED] rest"),
            ("SECRET=s3cr3t", "SECRET=[REDACTED]"),
            ("GITHUB_TOKEN = abc'", "GITHUB_TOKEN = [REDACTED]'"),
            (
                "{\"password\": \"pa ss\"}",
                "{\"password\": \"[REDACTED]\"}",
            ),
            ("Password:'x'", "Password:'[REDACTED]'"),
            (
                r#"{"token": "a\"b c", "n": 1}"#,
                r#"{"token": "[REDACTED]", "n": 1}"#,
            ),
            (
                "-----BEGIN PGP PRIVATE KEY BLOCK-----\nxyz\n-----END PGP PRIVATE KEY BLOCK-----",
                "[REDACTED]",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(red(input), *want, "input: {input:?}");
        }

        let untouched = [
            "task-123",
            "sketch",
            "ANTHROPIC_API_KEY is never read",
            "sk-short",
            "ask-abcdefghijklmnopqrstuvwxyz",
            "ghp_short",
            "github_pat_short",
            "xoxb-123",
            "xoxz-1234567890-abcdef",
            "AKIAIOSFODNN7EXAMPL",
            "AKIAIOSFODNN7EXAMPLEX",
            "-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----",
            "tokens: 1200",
            "secretary: Bob",
            "password:",
            "max_tokens = 5",
            "naïve café ünïcode — ok",
        ];
        for input in untouched {
            assert!(
                matches!(redact(input), Cow::Borrowed(_)),
                "changed: {input:?} -> {:?}",
                redact(input)
            );
        }
    }

    #[test]
    fn redaction_keeps_surrounding_unicode() {
        assert_eq!(
            red("é token=abc ü sk-ant-AAAAAAAAAA ß"),
            "é token=[REDACTED] ü [REDACTED] ß"
        );
    }

    #[test]
    fn env_values_redact_by_key() {
        assert_eq!(redact_env_value("ANTHROPIC_API_KEY", "x"), REDACTED);
        assert_eq!(redact_env_value("gh_token", "abc"), REDACTED);
        assert_eq!(redact_env_value("HOME", "/Users/me"), "/Users/me");
        assert_eq!(
            redact_env_value("NOTES", "use ghp_abcdefghijKLMNOPQRST0123"),
            "use [REDACTED]"
        );
        assert_eq!(redact_env_value("API_KEY", ""), "");
    }
}
