use common::{MarketEvent, OrderType, Side, TradeInstruction};
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

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
    // Initialize to a past time so the first trade is allowed immediately
    let mut last_trade_time = Instant::now() - Duration::from_secs(20);
    let mut next_side = Side::Buy; // Start with Buy

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

                // Removed logging from hot path for performance
                // tracing::info!(...);

                // Ping-Pong Logic: If price > 50,000, execute trade (throttled)
                let throttle_passed = disable_throttle || last_trade_time.elapsed() > Duration::from_secs(10);
                
                if event.price > 50_000.0 && throttle_passed {
                    let instr = TradeInstruction {
                        symbol: event.symbol.clone(),
                        side: next_side,
                        order_type: OrderType::Market,
                        price: event.price,
                        quantity: 0.01,
                        timestamp: common::now_nanos(),
                        dry_run, // Use the passed parameter
                    };

                    if let Err(e) = producer.push(instr) {
                        tracing::warn!("Failed to push instruction: {:?}", e);
                    } else {
                        last_trade_time = Instant::now();
                        // Toggle side for next trade
                        next_side = match next_side {
                            Side::Buy => Side::Sell,
                            Side::Sell => Side::Buy,
                        };
                        tracing::info!("Strategy: Switched next side to {:?}", next_side);
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
