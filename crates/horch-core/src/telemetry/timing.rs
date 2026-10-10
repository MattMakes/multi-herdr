//! What one collector tick spent, per phase, and what it wrote (NFR-02).
//!
//! The collector fills a [`TickTiming`] on every tick. With
//! `HORCH_TELEMETRY_TIMING=1` it prints the record to stderr after the tick.

use std::time::{Duration, Instant};

/// The phases of a tick, in tick order.
pub const PHASES: [&str; 9] = [
    "ledgers",
    "resolve",
    "read",
    "append",
    "cursor_save",
    "quota",
    "build",
    "serialize",
    "write",
];

/// The files a tick writes.
pub const FILES: [&str; 4] = ["events", "cursors", "quota", "snapshot"];

/// One tick's time per phase and bytes written per file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickTiming {
    pub phases: [Duration; PHASES.len()],
    pub bytes: [u64; FILES.len()],
    pub total: Duration,
}

impl TickTiming {
    /// Add the time since `start` to `phase`, and return now.
    pub(crate) fn lap(&mut self, phase: &str, start: Instant) -> Instant {
        let now = Instant::now();
        self.add(phase, now - start);
        now
    }

    pub(crate) fn add(&mut self, phase: &str, d: Duration) {
        if let Some(i) = PHASES.iter().position(|p| *p == phase) {
            self.phases[i] += d;
        }
    }

    pub(crate) fn wrote(&mut self, file: &str, bytes: u64) {
        if let Some(i) = FILES.iter().position(|f| *f == file) {
            self.bytes[i] += bytes;
        }
    }

    /// The time of `phase`.
    pub fn phase(&self, phase: &str) -> Duration {
        PHASES
            .iter()
            .position(|p| *p == phase)
            .map_or(Duration::ZERO, |i| self.phases[i])
    }

    /// The bytes written to `file`.
    pub fn written(&self, file: &str) -> u64 {
        FILES
            .iter()
            .position(|f| *f == file)
            .map_or(0, |i| self.bytes[i])
    }

    /// Every byte the tick wrote.
    pub fn written_total(&self) -> u64 {
        self.bytes.iter().sum()
    }

    /// One line: `tick 12.3ms ledgers=0.4ms ... | events=0B cursors=0B ...`.
    pub fn line(&self) -> String {
        let ms = |d: Duration| format!("{:.1}ms", d.as_secs_f64() * 1000.0);
        let mut out = format!("telemetry tick {}", ms(self.total));
        for (p, d) in PHASES.iter().zip(self.phases) {
            out.push_str(&format!(" {p}={}", ms(d)));
        }
        out.push_str(" |");
        for (f, b) in FILES.iter().zip(self.bytes) {
            out.push_str(&format!(" {f}={b}B"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfr_02_timing_adds_per_phase_and_per_file() {
        let mut t = TickTiming::default();
        t.add("read", Duration::from_millis(2));
        t.add("read", Duration::from_millis(3));
        t.add("no such phase", Duration::from_millis(7));
        t.wrote("cursors", 10);
        t.wrote("snapshot", 5);
        assert_eq!(t.phase("read"), Duration::from_millis(5));
        assert_eq!(t.written("cursors"), 10);
        assert_eq!(t.written_total(), 15);
        assert!(t.line().contains(" read=5.0ms"), "{}", t.line());
        assert!(t.line().contains(" cursors=10B"), "{}", t.line());
    }
}
