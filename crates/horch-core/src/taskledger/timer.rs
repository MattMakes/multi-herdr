//! The agent's on-screen "working for N" timer, read from a pane's visible
//! text (#31).
//!
//! The watchdog captures a worker pane (`herdr pane read`) and compares this
//! timer between checks: a timer that advanced by about the elapsed time
//! means the agent is still in one long turn. This module only parses text;
//! it does no I/O.
//!
//! Each known status-line shape is 1 entry in `FORMATS`. When a CLI release
//! changes its wording, add 1 entry and 1 fixture under
//! `tests/fixtures/screens/`.

/// The agent CLI a screen comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Any,
}

impl Agent {
    /// The agent CLI whose timer formats a fleet harness prints, or None for
    /// a harness with no known format (#203).
    pub fn of_harness(harness: &str) -> Option<Agent> {
        match harness {
            "codex" => Some(Agent::Codex),
            "claude" => Some(Agent::Claude),
            _ => None,
        }
    }
}

/// How long the agent's on-screen timer says it has been working, in seconds,
/// or None when the screen shows no running timer. Never Some(0) for "no timer".
///
/// Only a line that also says "esc to interrupt" is a running timer; a
/// finished turn ("Worked for 12m 30s") is not. With several timers on screen
/// the one nearest the bottom wins. [`Agent::Any`] tries every format; a named
/// agent tries its own formats first, then the others.
pub fn working_seconds(screen: &str, agent: Agent) -> Option<u64> {
    let own: Vec<&Format> = FORMATS
        .iter()
        .filter(|f| agent == Agent::Any || f.agent == agent)
        .collect();
    let others: Vec<&Format> = FORMATS
        .iter()
        .filter(|f| agent != Agent::Any && f.agent != agent)
        .collect();
    nearest_bottom(screen, &own).or_else(|| nearest_bottom(screen, &others))
}

/// The text every running status line carries (matched ignoring ASCII case).
const RUNNING: &str = "esc to interrupt";

/// The word before the parenthesised timer.
#[derive(Debug, Clone, Copy)]
enum Verb {
    /// Exactly one of these words.
    OneOf(&'static [&'static str]),
    /// Any alphabetic word followed by `…` or `...`.
    Ellipsis,
}

/// One status-line shape: `<glyph> <verb> (<duration><separator> ... esc to interrupt)`.
#[derive(Debug, Clone, Copy)]
struct Format {
    /// The CLI that prints it.
    agent: Agent,
    /// The word before `(`.
    verb: Verb,
    /// The character that ends the duration inside the parentheses.
    separator: char,
}

/// Known running-timer formats. Add 1 entry per new shape.
const FORMATS: &[Format] = &[
    // Codex CLI (~0.30 and later): `• Working (12m 30s • esc to interrupt)`.
    Format {
        agent: Agent::Codex,
        verb: Verb::OneOf(&["Working", "Thinking"]),
        separator: '•',
    },
    // Codex CLI (older, ~0.1x-0.2x): `Working (45s · esc to interrupt)`,
    // sometimes `Thinking` instead of `Working`.
    Format {
        agent: Agent::Codex,
        verb: Verb::OneOf(&["Working", "Thinking"]),
        separator: '·',
    },
    // Claude Code (~1.x-2.x) spinner line:
    // `✻ Thinking… (12m 30s · ↑ 4.2k tokens · esc to interrupt)`; the glyph
    // and the verb vary on every frame.
    Format {
        agent: Agent::Claude,
        verb: Verb::Ellipsis,
        separator: '·',
    },
];

/// The timer on the lowest line any of `formats` reads.
fn nearest_bottom(screen: &str, formats: &[&Format]) -> Option<u64> {
    screen
        .lines()
        .rev()
        .find_map(|line| formats.iter().find_map(|f| f.read(line)))
}

impl Format {
    /// The seconds on `line`, when `line` is a running timer in this format.
    fn read(&self, line: &str) -> Option<u64> {
        // ASCII lowercasing keeps byte offsets, so `esc` indexes `line` too.
        let esc = line.to_ascii_lowercase().rfind(RUNNING)?;
        let open = line[..esc].rfind('(')?;
        let verb = line[..open].trim_end().rsplit(char::is_whitespace).next()?;
        if !self.verb.accepts(verb) {
            return None;
        }
        let (duration, _) = line[open + 1..esc].split_once(self.separator)?;
        parse_duration(duration)
    }
}

impl Verb {
    fn accepts(self, word: &str) -> bool {
        match self {
            Verb::OneOf(words) => words.contains(&word),
            Verb::Ellipsis => word
                .strip_suffix('…')
                .or_else(|| word.strip_suffix("..."))
                .is_some_and(|w| !w.is_empty() && w.chars().all(char::is_alphabetic)),
        }
    }
}

/// `45s`, `12m`, `12m 30s`, `1h 02m 03s`, `1h 2m`: units in h, m, s order,
/// each at most once, zero padding allowed.
fn parse_duration(text: &str) -> Option<u64> {
    let mut rest = text.trim();
    let mut total: u64 = 0;
    let mut last_rank = 3;
    let mut any = false;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let n: u64 = rest[..digits].parse().ok()?;
        let (rank, unit) = match rest[digits..].chars().next()? {
            'h' => (2, 3600),
            'm' => (1, 60),
            's' => (0, 1),
            _ => return None,
        };
        if rank >= last_rank {
            return None;
        }
        last_rank = rank;
        total = total.checked_add(n.checked_mul(unit)?)?;
        rest = rest[digits + 1..].trim_start();
        any = true;
    }
    any.then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! screen {
        ($name:literal) => {
            include_str!(concat!("../../tests/fixtures/screens/", $name, ".txt"))
        };
    }

    #[test]
    fn codex_screens() {
        let cases = [
            (screen!("codex_seconds"), Some(45)),
            (screen!("codex_minutes"), Some(12 * 60 + 30)),
            (screen!("codex_hours"), Some(3600 + 2 * 60 + 3)),
            (screen!("codex_old_thinking"), Some(7 * 60 + 5)),
        ];
        for (screen, want) in cases {
            for agent in [Agent::Codex, Agent::Any, Agent::Claude] {
                assert_eq!(working_seconds(screen, agent), want, "{agent:?}\n{screen}");
            }
        }
    }

    #[test]
    fn claude_screens() {
        let cases = [
            (screen!("claude_seconds"), Some(45)),
            (screen!("claude_minutes"), Some(12 * 60 + 30)),
            (screen!("claude_hours"), Some(3600 + 2 * 60 + 3)),
        ];
        for (screen, want) in cases {
            for agent in [Agent::Claude, Agent::Any, Agent::Codex] {
                assert_eq!(working_seconds(screen, agent), want, "{agent:?}\n{screen}");
            }
        }
    }

    #[test]
    fn two_timers_take_the_bottom_one() {
        let screen = screen!("claude_two_timers");
        assert_eq!(working_seconds(screen, Agent::Claude), Some(3 * 60 + 12));
        assert_eq!(working_seconds(screen, Agent::Any), Some(3 * 60 + 12));
    }

    #[test]
    fn no_running_timer() {
        for screen in [
            screen!("claude_worked_for"),
            screen!("codex_worked_for"),
            screen!("shell"),
        ] {
            for agent in [Agent::Claude, Agent::Codex, Agent::Any] {
                assert_eq!(working_seconds(screen, agent), None, "{agent:?}\n{screen}");
            }
        }
    }

    #[test]
    fn named_agent_prefers_its_own_format() {
        // A Claude line low on screen, a Codex line above it.
        let screen = "• Working (5m 00s • esc to interrupt)\n\
                      ✻ Thinking… (10s · esc to interrupt)\n";
        assert_eq!(working_seconds(screen, Agent::Codex), Some(300));
        assert_eq!(working_seconds(screen, Agent::Claude), Some(10));
        assert_eq!(working_seconds(screen, Agent::Any), Some(10));
    }

    #[test]
    fn single_lines() {
        let cases = [
            ("• Working (12m 30s • esc to interrupt)", Some(750)),
            ("Working (45s • esc to interrupt)", Some(45)),
            ("Working (1h 02m 03s • esc to interrupt)", Some(3723)),
            ("· Thinking (2m · esc to interrupt)", Some(120)),
            ("Working (0s • esc to interrupt)", Some(0)),
            ("· Pondering… (45s · esc to interrupt)", Some(45)),
            (
                "✢ Brewing… (1h 2m 3s · ↓ 12k tokens · esc to interrupt)",
                Some(3723),
            ),
            ("✶ Cogitating... (1h 5m · esc to interrupt)", Some(3900)),
            (
                "✻ Thinking… (12m 30s · ↑ 4.2k tokens · Esc to interrupt)",
                Some(750),
            ),
            ("✻ Worked for 12m 30s", None),
            ("✻ Cooked for 3m", None),
            ("Thinking… (esc to interrupt)", None),
            ("Working (soon • esc to interrupt)", None),
            ("Working (30s 2m • esc to interrupt)", None),
            (
                "Thinking (12m 30s · esc to interrupt) no ellipsis but Codex-old",
                Some(750),
            ),
            ("$ echo 'press esc to interrupt'", None),
        ];
        for (line, want) in cases {
            assert_eq!(working_seconds(line, Agent::Any), want, "{line}");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("45s"), Some(45));
        assert_eq!(parse_duration("12m"), Some(720));
        assert_eq!(parse_duration("12m 30s"), Some(750));
        assert_eq!(parse_duration("01h 02m 03s"), Some(3723));
        assert_eq!(parse_duration("1h 2m"), Some(3720));
        assert_eq!(parse_duration(" 1h02m03s "), Some(3723));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("1m 1m"), None);
        assert_eq!(parse_duration("1x"), None);
        assert_eq!(parse_duration("m"), None);
    }
}
