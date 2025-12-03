use common::{MarketEvent, TradeInstruction};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

mod momentum;
use momentum::MomentumStrategy;

mod ping_pong;
use ping_pong::PingPongStrategy;

mod liquidation;
use liquidation::LiquidationStrategy;

include!(concat!(env!("OUT_DIR"), "/strategies.rs"));

pub trait Strategy: Send {
    fn process_event(&mut self, event: &MarketEvent) -> Option<TradeInstruction>;
}

fn create_strategy(
    name: &str,
    fee_maker: f64,
    fee_taker: f64,
    window: usize,
    threshold: f64,
    price_threshold: f64,
    volume_multiplier: f64,
) -> Box<dyn Strategy> {
    match name {
        "PING_PONG" => Box::new(PingPongStrategy::new(false)),
        "MOMENTUM" => Box::new(MomentumStrategy::new(
            window, threshold, fee_maker, fee_taker,
        )),
        "LIQUIDATION" => Box::new(LiquidationStrategy::new(price_threshold, volume_multiplier)),
        _ => {
            tracing::warn!("Unknown strategy: {}, defaulting to PING_PONG", name);
            Box::new(PingPongStrategy::new(false))
        }
    }
}

/// Runs the synchronous strategy consumer loop on the current OS thread.
/// This function MUST NOT return under normal operation; it should read from the consumer
/// forever until `shutdown` is set to true.
pub fn run(
    mut consumer: Consumer<MarketEvent>,
    mut producer: Producer<TradeInstruction>,
    shutdown: Arc<AtomicBool>,
    is_running: Arc<AtomicBool>,
    active_strategy: Arc<Mutex<String>>,
    _dry_run: bool,
    _disable_throttle: bool,
    fee_maker: f64,
    fee_taker: f64,
    strategy_window: usize,
    strategy_threshold: f64,
    price_threshold: f64,
    volume_multiplier: f64,
) {
    tracing::info!("Strategy thread started");

    // Initialize Strategy
    let mut current_strategy_name = active_strategy.lock().clone();
    let mut strategy = create_strategy(
        &current_strategy_name,
        fee_maker,
        fee_taker,
        strategy_window,
        strategy_threshold,
        price_threshold,
        volume_multiplier,
    );
    tracing::info!("Active Strategy: {}", current_strategy_name);

    while !shutdown.load(Ordering::Relaxed) {
        // Check if engine is running
        if !is_running.load(Ordering::Relaxed) {
            std::thread::yield_now();
            continue;
        }

        // Check for strategy change
        if let Some(guard) = active_strategy.try_lock() {
            if *guard != current_strategy_name {
                current_strategy_name = guard.clone();
                strategy = create_strategy(
                    &current_strategy_name,
                    fee_maker,
                    fee_taker,
                    strategy_window,
                    strategy_threshold,
                    price_threshold,
                    volume_multiplier,
                );
                tracing::info!("Switched Strategy to: {}", current_strategy_name);
            }
        }

        match consumer.pop() {
            Ok(event) => {
                let now = common::now_nanos();
                let _latency_ns = now.saturating_sub(event.received_timestamp);

                // Process Event via Strategy
                if let Some(instr) = strategy.process_event(&event) {
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
