//! `timings` (0.3.46, deno study A9): where a validate run spends its
//! time, on request. `HARNA_TIMINGS=1 harna validate` prints each phase's
//! wall time to stderr, slowest first. A full validate took ~2 minutes on
//! deno's 1.2M lines; hooks on big repos need to know which check to make
//! cheaper, not guess.

use std::time::{Duration, Instant};

pub struct Timer {
    on: bool,
    last: Instant,
    phases: Vec<(&'static str, Duration)>,
}

impl Timer {
    pub fn from_env() -> Timer {
        Timer {
            on: std::env::var("HARNA_TIMINGS").is_ok_and(|v| v != "0" && !v.is_empty()),
            last: Instant::now(),
            phases: Vec::new(),
        }
    }

    /// Close the phase that ran since the previous mark.
    pub fn mark(&mut self, phase: &'static str) {
        if !self.on {
            return;
        }
        let now = Instant::now();
        // as it happens too (0.9.1): a slow run shows where it is
        eprintln!(
            "[validate] {phase}: {:.2}s",
            (now - self.last).as_secs_f64()
        );
        self.phases.push((phase, now - self.last));
        self.last = now;
    }

    /// The report, slowest phase first ("" when timings are off).
    pub fn report(&self) -> String {
        if !self.on || self.phases.is_empty() {
            return String::new();
        }
        let total: Duration = self.phases.iter().map(|(_, d)| *d).sum();
        let mut sorted = self.phases.clone();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
        let mut out = format!("validate timings — {:.2}s total\n", total.as_secs_f64());
        for (phase, d) in sorted {
            out.push_str(&format!("  {:>8.2}s  {phase}\n", d.as_secs_f64()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_are_silent_unless_asked_and_sorted_when_on() {
        let mut off = Timer {
            on: false,
            last: Instant::now(),
            phases: Vec::new(),
        };
        off.mark("x");
        assert_eq!(off.report(), "");
        let mut on = Timer {
            on: true,
            last: Instant::now(),
            phases: Vec::new(),
        };
        on.phases.push(("fast", Duration::from_millis(5)));
        on.phases.push(("slow", Duration::from_millis(50)));
        let r = on.report();
        assert!(r.starts_with("validate timings"));
        assert!(r.find("slow").unwrap() < r.find("fast").unwrap(), "{r}");
    }
}
