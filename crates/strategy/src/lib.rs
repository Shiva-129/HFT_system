use common::{MarketEvent, TradeInstruction, Side, OrderType};
use rtrb::{Consumer, Producer, PushError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Runs the synchronous strategy consumer loop on the current OS thread.
/// This function MUST NOT return under normal operation; it should read from the consumer
/// forever until `shutdown` is set to true.
pub fn run(
    mut consumer: Consumer<MarketEvent>,
    mut producer: Producer<TradeInstruction>,
    shutdown: Arc<AtomicBool>,
) {
    tracing::info!("Strategy thread started");

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
                
                // Ping-Pong Logic: If price > 50,000, buy 0.01
                if event.price > 50_000.0 {
                    let instr = TradeInstruction {
                        symbol: event.symbol.clone(),
                        side: Side::Buy,
                        order_type: OrderType::Limit,
                        price: event.price,
                        quantity: 0.01,
                        timestamp: common::now_nanos(),
                        dry_run: true,
                    };
                    
                    match producer.push(instr) {
                        Ok(_) => {}
                        Err(PushError::Full(_)) => {
                            tracing::warn!("Output Queue Full - Dropping TradeInstruction");
                        }
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
