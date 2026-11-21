use std::time::{Duration, Instant};

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

pub const FILES: [&str; 4] = ["events", "cursors", "quota", "snapshot"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickTiming {
    pub phases: [Duration; PHASES.len()],
    pub bytes: [u64; FILES.len()],
    pub total: Duration,
}

impl TickTiming {
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

    pub fn phase(&self, phase: &str) -> Duration {
        PHASES
            .iter()
            .position(|p| *p == phase)
            .map_or(Duration::ZERO, |i| self.phases[i])
    }

    pub fn written(&self, file: &str) -> u64 {
        FILES
            .iter()
            .position(|f| *f == file)
            .map_or(0, |i| self.bytes[i])
    }

    pub fn written_total(&self) -> u64 {
        self.bytes.iter().sum()
    }

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
