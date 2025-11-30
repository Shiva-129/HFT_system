use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    tracing::info!("Starting Trading Engine...");

    // Create Ring Buffer (Capacity 4096 - Power of two for efficiency)
    let (mut producer, consumer) = rtrb::RingBuffer::<common::MarketEvent>::new(4096);

    // Shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    // Install Ctrl+C handler
    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Received Ctrl+C, initiating shutdown...");
                shutdown_signal.store(true, Ordering::Relaxed);
            }
            Err(err) => {
                tracing::error!("Unable to listen for shutdown signal: {}", err);
            }
        }
    });

    // Start Strategy Thread (OS thread)
    let strategy_handle = std::thread::spawn(move || {
        strategy::run(consumer, shutdown_clone);
    });

    // Start Feed Task (Tokio)
    // We connect to a hardcoded symbol for now as per requirements
    let symbol = "btcusdt";
    tracing::info!("Connecting to feed for {}...", symbol);

    // Note: connect returns a Receiver. We need to bridge this to the ring buffer.
    // We pass None for raw_tx as we don't need raw recording here.
    let mut feed_rx = feed_handler::connect(symbol, None).await?;

    let feed_shutdown = shutdown.clone();

    // Feed processing loop
    while !feed_shutdown.load(Ordering::Relaxed) {
        // Use timeout to allow checking shutdown flag periodically if no ticks come in
        match tokio::time::timeout(Duration::from_millis(100), feed_rx.recv()).await {
            Ok(Some(event)) => {
                // Attempt to push into ring buffer (non-blocking)
                match producer.push(event) {
                    Ok(_) => {
                        // Success
                    }
                    Err(_) => {
                        // Ring buffer full - shed load
                        tracing::warn!("ring buffer full — dropping tick");
                    }
                }
            }
            Ok(None) => {
                tracing::warn!("Feed channel closed unexpectedly");
                break;
            }
            Err(_) => {
                // Timeout, just loop back to check shutdown
                continue;
            }
        }
    }

    tracing::info!("Feed loop exited. Waiting for strategy thread to join...");

    // Wait for strategy thread to finish (it should exit when it sees shutdown flag)
    // We give it a bit of time to drain if needed, but strategy::run checks shutdown constantly
    match strategy_handle.join() {
        Ok(_) => tracing::info!("Strategy thread joined successfully."),
        Err(e) => tracing::error!("Strategy thread panicked: {:?}", e),
    }

    tracing::info!("Trading Engine shutdown complete.");
    Ok(())
}
