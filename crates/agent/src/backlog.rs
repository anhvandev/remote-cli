//! Resource samples not yet sent to the Server. While disconnected it keeps the last hour; older samples drop first.

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

use rcli_protocol::agent::Resources;
use tokio::sync::Notify;

pub const KEEP: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Backlog {
    samples: Mutex<VecDeque<(Instant, Resources)>>,
    ready: Notify,
}

impl Backlog {
    /// Adds a complete sample taken at `at`, dropping samples more than `KEEP` older than it.
    pub fn push(&self, at: Instant, sample: Resources) {
        let mut samples = self.samples.lock().unwrap();
        while samples.front().is_some_and(|(t, _)| at.saturating_duration_since(*t) > KEEP) {
            samples.pop_front();
        }
        samples.push_back((at, sample));
        self.ready.notify_one();
    }

    /// Removes and returns every waiting sample, oldest first.
    pub fn take(&self) -> Vec<(Instant, Resources)> {
        self.samples.lock().unwrap().drain(..).collect()
    }

    /// Resolves once a sample has been pushed since the last call.
    pub async fn ready(&self) {
        self.ready.notified().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(c: f64) -> Resources {
        Resources { cpu_percent: Some(c), ..Default::default() }
    }

    #[test]
    fn keeps_one_hour_and_drops_the_oldest_first() {
        let backlog = Backlog::default();
        let t0 = Instant::now();
        for i in 0..=720 {
            backlog.push(t0 + Duration::from_secs(5 * i), cpu(i as f64));
        }
        let kept = backlog.take();
        assert_eq!(kept.len(), 721, "exactly one hour (0..=3600 s) is kept");
        backlog.push(t0, cpu(0.0));
        backlog.push(t0 + KEEP + Duration::from_secs(5), cpu(1.0));
        backlog.push(t0 + KEEP + Duration::from_secs(10), cpu(2.0));
        let kept: Vec<_> = backlog.take().into_iter().map(|(_, r)| r.cpu_percent.unwrap()).collect();
        assert_eq!(kept, [1.0, 2.0]);
        assert!(backlog.take().is_empty());
    }
}
