use chrono::{DateTime, TimeZone, Utc};
use std::sync::Mutex;

/// Source of "now" for every time-based rule, so tests can move time explicitly.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Time only moves when `advance` is called.
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl Default for ManualClock {
    fn default() -> Self {
        Self(Mutex::new(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()))
    }
}

impl ManualClock {
    pub fn advance(&self, by: chrono::Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}
