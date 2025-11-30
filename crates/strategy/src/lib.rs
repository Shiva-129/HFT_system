use common::MarketEvent;
use rtrb::Consumer;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Runs the synchronous strategy consumer loop on the current OS thread.
/// This function MUST NOT return under normal operation; it should read from the consumer
/// forever until `shutdown` is set to true.
pub fn run(mut consumer: Consumer<MarketEvent>, shutdown: Arc<AtomicBool>) {
    tracing::info!("Strategy thread started");

    while !shutdown.load(Ordering::Relaxed) {
        match consumer.pop() {
            Ok(event) => {
                let now_nanos = common::time::MONOTONIC_START.elapsed().as_nanos() as u64;
                let latency_ns = now_nanos.saturating_sub(event.received_timestamp);

                tracing::info!(
                    symbol = %event.symbol,
                    price = event.price,
                    latency_ns = latency_ns,
                    "tick received"
                );

                // Future: Strategy logic goes here
            }
            Err(_) => {
                // Buffer is empty, yield to avoid 100% CPU on dev machines
                std::thread::yield_now();
            }
        }
    }

    tracing::info!("Strategy thread shutting down");
}
