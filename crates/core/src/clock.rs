//! Time as a dependency (`architecture.md` §4): the `Session` owns a [`Clock`]; debouncing,
//! "Recently Updated" and age filters read it. Tests use a [`VirtualClock`] they advance by hand.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (UTC). Arithmetic saturates instead of overflowing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const EPOCH: Timestamp = Timestamp(0);

    pub const fn from_millis(ms: i64) -> Timestamp {
        Timestamp(ms)
    }

    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// Whole seconds since the epoch, rounded towards negative infinity.
    pub const fn as_secs(self) -> i64 {
        self.0.div_euclid(1000)
    }

    pub fn saturating_add(self, d: Duration) -> Timestamp {
        Timestamp(self.0.saturating_add(millis(d)))
    }

    pub fn saturating_sub(self, d: Duration) -> Timestamp {
        Timestamp(self.0.saturating_sub(millis(d)))
    }

    /// The time from `earlier` to `self`; zero when `earlier` is later.
    pub fn since(self, earlier: Timestamp) -> Duration {
        let ms = self.0.saturating_sub(earlier.0).max(0);
        Duration::from_millis(u64::try_from(ms).unwrap_or(0))
    }

    /// Converts a file-system time; times before the epoch become negative.
    pub fn from_system_time(t: SystemTime) -> Timestamp {
        match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Timestamp(millis(d)),
            Err(e) => Timestamp(millis(e.duration()).saturating_neg()),
        }
    }
}

/// A duration in whole milliseconds, saturated to `i64::MAX`.
fn millis(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

/// The source of "now". Shared across threads behind an `Arc<dyn Clock>`.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// The operating system's wall clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_system_time(SystemTime::now())
    }
}

/// A clock that only moves when told to. Deterministic tests and the headless driver use it.
#[derive(Debug, Default)]
pub struct VirtualClock {
    now: AtomicI64,
}

impl VirtualClock {
    pub fn new(start: Timestamp) -> VirtualClock {
        VirtualClock { now: AtomicI64::new(start.0) }
    }

    pub fn advance(&self, d: Duration) {
        let step = millis(d);
        // A compare-and-swap loop (what `fetch_update` does), written out so it builds
        // warning-free on toolchains before and after its rename to `try_update`.
        let mut current = self.now.load(Ordering::SeqCst);
        while let Err(seen) = self.now.compare_exchange_weak(current, current.saturating_add(step), Ordering::SeqCst, Ordering::SeqCst) {
            current = seen;
        }
    }

    pub fn set(&self, t: Timestamp) {
        self.now.store(t.0, Ordering::SeqCst);
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn virtual_clock_moves_only_when_told() {
        let clock = VirtualClock::new(Timestamp::from_millis(1_000));
        assert_eq!(clock.now().as_millis(), 1_000);
        clock.advance(Duration::from_millis(1_500));
        assert_eq!(clock.now().as_millis(), 2_500);
        clock.set(Timestamp::EPOCH);
        assert_eq!(clock.now(), Timestamp::EPOCH);
        clock.advance(Duration::MAX);
        assert_eq!(clock.now().as_millis(), i64::MAX);
        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.now().as_millis(), i64::MAX);
    }

    #[test]
    fn clocks_are_shareable_as_trait_objects() {
        let clocks: Vec<Arc<dyn Clock>> = vec![Arc::new(SystemClock), Arc::new(VirtualClock::default())];
        assert!(clocks[0].now() > Timestamp::from_millis(1_700_000_000_000));
        assert_eq!(clocks[1].now(), Timestamp::EPOCH);
    }

    #[test]
    fn timestamp_arithmetic_saturates() {
        let t = Timestamp::from_millis(5_000);
        assert_eq!(t.saturating_add(Duration::from_millis(250)).as_millis(), 5_250);
        assert_eq!(t.saturating_sub(Duration::from_secs(10)).as_millis(), -5_000);
        assert_eq!(Timestamp::from_millis(i64::MIN).saturating_sub(Duration::from_secs(1)).as_millis(), i64::MIN);
        assert_eq!(t.since(Timestamp::from_millis(2_000)), Duration::from_secs(3));
        assert_eq!(Timestamp::from_millis(2_000).since(t), Duration::ZERO);
        assert_eq!(Timestamp::from_millis(i64::MAX).since(Timestamp::from_millis(i64::MIN)), Duration::from_millis(i64::MAX as u64));
        assert_eq!(Timestamp::from_millis(-1).as_secs(), -1);
    }

    #[test]
    fn system_times_before_the_epoch_are_negative() {
        let before = UNIX_EPOCH - Duration::from_millis(1_500);
        assert_eq!(Timestamp::from_system_time(before).as_millis(), -1_500);
        assert_eq!(serde_json::to_string(&Timestamp::from_millis(-1_500)).unwrap(), "-1500");
    }
}
