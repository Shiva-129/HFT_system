use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize Telemetry (Logging)
    // Must be done before any other logging or signal handlers.
    // The guard must be kept alive to ensure logs are flushed.
    let _guard = telemetry::init("./logs");

    tracing::info!("Starting Trading Engine...");



    // Create Ring Buffer (Capacity 4096 - Power of two for efficiency)
    let (mut producer, consumer) = rtrb::RingBuffer::<common::MarketEvent>::new(4096);
    
    // Create Signal Ring Buffer (Strategy -> Execution)
    let (signal_producer, _signal_consumer) = rtrb::RingBuffer::<common::TradeInstruction>::new(4096);

    // Shutdown signal for the main loop
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    // We need a way to set shutdown=true when Ctrl+C happens.
    // Since `ctrlc` crate takes a closure `FnMut`, and we want to capture `shutdown`, we can do that!
    // `ctrlc::set_handler` takes `F: Fn() -> () + Send + Sync + 'static`.
    // `Arc` is Send + Sync. So we CAN move a clone of shutdown into the handler!
    let shutdown_signal = shutdown.clone();
    ctrlc::set_handler(move || {
        tracing::warn!(">>>> CTRL+C RECEIVED <<<<   DISARMING TRADING SYSTEM IMMEDIATELY");
        risk_engine::disarm();
        shutdown_signal.store(true, Ordering::Relaxed);
    }).expect("Error setting Ctrl-C handler");


    // Start Strategy Thread (OS thread)
    let strategy_handle = std::thread::spawn(move || {
        strategy::run(consumer, signal_producer, shutdown_clone);
    });

    // Start Feed Task (Tokio)
    let symbol = "btcusdt";
    tracing::info!("Connecting to feed for {}...", symbol);
    
    let mut feed_rx = feed_handler::connect(symbol, None).await?;

    // ARM THE SYSTEM
    // This ensures no accidental orders slip through while the system is still booting.
    risk_engine::arm();

    let feed_shutdown = shutdown.clone();
    
    // Feed processing loop
    while !feed_shutdown.load(Ordering::Relaxed) {
        match tokio::time::timeout(Duration::from_millis(100), feed_rx.recv()).await {
            Ok(Some(event)) => {
                match producer.push(event) {
                    Ok(_) => {}
                    Err(_) => {
                        tracing::warn!("ring buffer full — dropping tick");
                    }
                }
            }
            Ok(None) => {
                tracing::warn!("Feed channel closed unexpectedly");
                break;
            }
            Err(_) => {
                continue;
            }
        }
    }

    tracing::info!("Feed loop exited. Waiting for strategy thread to join...");

    match strategy_handle.join() {
        Ok(_) => tracing::info!("Strategy thread joined successfully."),
        Err(e) => tracing::error!("Strategy thread panicked: {:?}", e),
    }

    tracing::info!("Trading Engine shutdown complete.");
    Ok(())
}
