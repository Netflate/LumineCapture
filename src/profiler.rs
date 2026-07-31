use std::time::{Duration, Instant};

/// LUMINE_PROFILE=1 cargo run --release
pub struct Profiler {
    t0: Instant,
    last: Instant,
    enabled: bool,
    lines: Vec<String>,
}

impl Profiler {
    pub fn new() -> Self {
        let enabled = std::env::var_os("LUMINE_PROFILE").is_some();
        let now = Instant::now();
        Self { t0: now, last: now, enabled, lines: Vec::new() }
    }

    pub fn mark(&mut self, label: &str) {
        if !self.enabled { return; }
        let now = Instant::now();
        self.lines.push(format!(
            "[{:>6.2}ms] (+{:>5.2}ms) {label}",
            self.t0.elapsed().as_secs_f64() * 1000.0,
            now.duration_since(self.last).as_secs_f64() * 1000.0,
        ));
        self.last = now;
    }

    pub fn mark_external(&mut self, label: &str, elapsed_since_t0: Duration) {
        if !self.enabled { return; }
        self.lines.push(format!(
            "[{:>6.2}ms] (thread)   {label}",
            elapsed_since_t0.as_secs_f64() * 1000.0,
        ));
    }

    pub fn dump(&self) {
        if !self.enabled || self.lines.is_empty() { return; }
        eprintln!("--- timing ---\n{}", self.lines.join("\n"));
    }

    pub fn t0(&self) -> Instant {
        self.t0
    }
}