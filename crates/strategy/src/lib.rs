use common::{MarketEvent, TradeInstruction};
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

mod ping_pong;
use ping_pong::PingPongStrategy;

include!(concat!(env!("OUT_DIR"), "/strategies.rs"));

/// Runs the synchronous strategy consumer loop on the current OS thread.
/// This function MUST NOT return under normal operation; it should read from the consumer
/// forever until `shutdown` is set to true.
pub fn run(
    mut consumer: Consumer<MarketEvent>,
    mut producer: Producer<TradeInstruction>,
    shutdown: Arc<AtomicBool>,
    is_running: Arc<AtomicBool>,
    dry_run: bool,
    disable_throttle: bool,
) {
    tracing::info!("Strategy thread started");
    
    // Initialize Strategy
    let mut strategy = PingPongStrategy::new(dry_run);

    while !shutdown.load(Ordering::Relaxed) {
        // Check if engine is running
        if !is_running.load(Ordering::Relaxed) {
            std::thread::yield_now();
            continue;
        }

        match consumer.pop() {
            Ok(event) => {
                let now = common::now_nanos();
                let _latency_ns = now.saturating_sub(event.received_timestamp);

                // Process Event via Strategy
                if let Some(instr) = strategy.process_event(&event, disable_throttle) {
                    if let Err(e) = producer.push(instr) {
                        tracing::warn!("Failed to push instruction: {:?}", e);
                    }
                }
            }
            Err(_) => {
                // Buffer is empty, yield to avoid 100% CPU on dev machines
                std::thread::yield_now();
            }
        }
    }

    tracing::info!("Strategy thread shutting down");
}
