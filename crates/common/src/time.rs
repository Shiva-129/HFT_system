use once_cell::sync::Lazy;
use std::time::Instant;

/// Global monotonic start time for the application.
/// Used to calculate relative timestamps for latency measurements.
pub static MONOTONIC_START: Lazy<Instant> = Lazy::new(Instant::now);
