mod config;

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tracing_subscriber::fmt::format::FmtSpan;
use execution::ExecutionClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load Config
    let config = match config::load("config.toml") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("CRITICAL: {}", e);
            eprintln!("Please copy config.example.toml to config.toml and configure it.");
            std::process::exit(1);
        }
    };

    // 2. Initialize Telemetry
    let _guard = telemetry::init("./logs");
    tracing::info!("Starting Trading Engine...");
    tracing::info!("Config loaded: Network={}, DryRun={}", config.network.name, config.trading.dry_run);

    // 3. Initialize Execution Client
    let api_key = config.trading.api_key.clone().unwrap_or_default();
    let secret_key = config.trading.secret_key.clone().unwrap_or_default();
    
    if config.trading.enabled && (api_key.is_empty() || secret_key.is_empty()) {
        tracing::error!("Trading enabled but API keys missing!");
        std::process::exit(1);
    }

    let execution_client = Arc::new(ExecutionClient::new(
        api_key, 
        secret_key, 
        config.network.rest_url.clone()
    ));

    // 4. Position Sync (The "Am I Holding the Bag?" Check)
    tracing::info!("Syncing positions...");
    match execution_client.get_position_risk().await {
        Ok(positions) => tracing::info!("Positions synced: {}", positions),
        Err(e) => tracing::warn!("Failed to sync positions: {}", e),
    }

    // 5. Setup Ring Buffers
    // Market Data: Feed -> Strategy
    let (mut producer, consumer) = rtrb::RingBuffer::<common::MarketEvent>::new(4096);
    // Signals: Strategy -> Execution
    let (signal_producer, mut signal_consumer) = rtrb::RingBuffer::<common::TradeInstruction>::new(4096);

    // 6. Shutdown Signals
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    
    // Create a notification channel for async tasks to know when to stop
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let mut shutdown_rx_execution = shutdown_tx.subscribe();

    // Ctrl+C Handler
    let shutdown_signal = shutdown.clone();
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    ctrlc::set_handler(move || {
        tracing::warn!(">>>> CTRL+C RECEIVED <<<<   DISARMING TRADING SYSTEM IMMEDIATELY");
        risk_engine::disarm();
        shutdown_signal.store(true, Ordering::Relaxed);
        let _ = shutdown_tx_ctrlc.send(());
    }).expect("Error setting Ctrl-C handler");

    // 7. Spawn Strategy Thread (Sync OS Thread)
    let strategy_handle = std::thread::spawn(move || {
        strategy::run(consumer, signal_producer, shutdown_clone);
    });

    // 8. Spawn Execution Task (Async Tokio Task)
    let execution_client_task = execution_client.clone();
    let execution_handle = tokio::spawn(async move {
        tracing::info!("Execution task started");
        loop {
            // Check for shutdown signal
            if shutdown_rx_execution.try_recv().is_ok() {
                break;
            }

            match signal_consumer.pop() {
                Ok(instruction) => {
                    tracing::info!("Received instruction: {:?}", instruction);
                    match execution_client_task.place_order(&instruction).await {
                        Ok(response) => tracing::info!("Order Placed: {}", response),
                        Err(e) => tracing::error!("Order Failed: {}", e),
                    }
                }
                Err(_) => {
                    // Queue empty, yield
                    tokio::task::yield_now().await;
                }
            }
        }
        tracing::info!("Execution task shutting down");
    });

    // 9. Spawn Feed Task (Tokio)
    let mut shutdown_rx_feed = shutdown_tx.subscribe();
    let feed_handle = tokio::spawn(async move {
        // Placeholder for feed connection
        tracing::info!("Feed task started (placeholder)");
        // Wait for shutdown signal
        let _ = shutdown_rx_feed.recv().await;
        tracing::info!("Feed task shutting down");
    });

    // 10. Wait for Shutdown
    if let Err(e) = strategy_handle.join() {
        tracing::error!("Strategy thread panicked: {:?}", e);
    }
    
    let _ = tokio::join!(execution_handle, feed_handle);

    // 11. Cancel-On-Exit (The Dead Man's Switch)
    tracing::warn!("Attempting to cancel all open orders...");
    match execution_client.cancel_all_orders("BTCUSDT").await {
        Ok(resp) => tracing::info!("Cancel All response: {}", resp),
        Err(e) => tracing::error!("Failed to cancel orders: {}", e),
    }

    tracing::info!("Trading Engine shutdown complete.");
    Ok(())
}
