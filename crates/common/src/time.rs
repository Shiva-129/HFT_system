use once_cell::sync::Lazy;
use std::time::Instant;

/// Global monotonic start time for the application.
/// Used to calculate relative timestamps for latency measurements.
pub static MONOTONIC_START: Lazy<Instant> = Lazy::new(Instant::now);

/// Returns the current monotonic time in nanoseconds since `MONOTONIC_START`.
pub fn now_nanos() -> u64 {
    MONOTONIC_START.elapsed().as_nanos() as u64
}
