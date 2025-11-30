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
) {
    tracing::info!("Strategy thread started");
    // Initialize to a past time so the first trade is allowed immediately
    let mut last_trade_time = Instant::now() - Duration::from_secs(20);

    while !shutdown.load(Ordering::Relaxed) {
        match consumer.pop() {
            Ok(event) => {
                let now = common::now_nanos();
                let latency_ns = now.saturating_sub(event.received_timestamp);

                tracing::info!(
                    symbol = %event.symbol,
                    price = event.price,
                    latency_ns = latency_ns,
                    "event processed"
                );

                // Ping-Pong Logic: If price > 50,000, buy 0.01 (throttled)
                if event.price > 50_000.0 && last_trade_time.elapsed() > Duration::from_secs(10) {
                    let instr = TradeInstruction {
                        symbol: event.symbol.clone(),
                        side: Side::Buy,
                        order_type: OrderType::Market,
                        price: event.price,
                        quantity: 0.01,
                        timestamp: common::now_nanos(),
                        dry_run: true, // Safety first
                    };
                    
                    if let Err(e) = producer.push(instr) {
                        tracing::warn!("Failed to push instruction: {:?}", e);
                    } else {
                        last_trade_time = Instant::now();
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
