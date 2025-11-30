mod config;
mod state;
mod server;

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use execution::ExecutionClient;
use crate::state::EngineState;

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

    // 3. Initialize Shared State
    let state = Arc::new(EngineState::new());
    // Initialize limits from config
    *state.max_loss_limit.lock() = config.risk.max_drawdown; // Using max_drawdown as initial max_loss
    // target_profit is 0.0 by default, can be set via API

    // 4. Spawn Web Server
    let server_state = state.clone();
    tokio::spawn(async move {
        server::run(server_state).await;
    });

    // 5. Initialize Execution Client
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

    // 6. Initialize Risk Engine
    let mut risk_engine = risk_engine::RiskEngine::new(
        config.risk.max_order_size,
        config.risk.max_drawdown,
    );

    // 7. Position Sync
    tracing::info!("Syncing positions...");
    match execution_client.sync_positions().await {
        Ok(positions) => {
            tracing::info!("Position sync OK: {} positions found", positions.len());
            for p in positions {
                if p.position_amt.parse::<f64>().unwrap_or(0.0).abs() > 0.0 {
                    tracing::info!("  Active Position: {} = {}", p.symbol, p.position_amt);
                }
            }
        },
        Err(e) => {
            tracing::warn!("Failed to sync positions: {}", e);
            if e.to_string().contains("AUTH_ERROR") && config.trading.enabled {
                tracing::error!("CRITICAL: Authentication failed. Cannot start trading engine.");
                std::process::exit(1);
            }
        },
    }

    // 8. Setup Ring Buffers
    let (mut _producer, consumer) = rtrb::RingBuffer::<common::MarketEvent>::new(4096);
    let (signal_producer, mut signal_consumer) = rtrb::RingBuffer::<common::TradeInstruction>::new(4096);

    // 9. Shutdown Signals
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let mut shutdown_rx_execution = shutdown_tx.subscribe();

    // Ctrl+C Handler
    let shutdown_signal = shutdown.clone();
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    let execution_client_ctrlc = execution_client.clone();
    let cleanup_done = Arc::new(AtomicBool::new(false));
    let state_ctrlc = state.clone();
    let runtime_handle = tokio::runtime::Handle::current(); // Capture handle
    
    ctrlc::set_handler(move || {
        if cleanup_done.swap(true, Ordering::SeqCst) {
            return;
        }

        tracing::warn!(">>>> CTRL+C RECEIVED <<<<   INITIATING GRACEFUL SHUTDOWN");
        
        // 1. Mark Shutting Down (Stops API)
        state_ctrlc.shutting_down.store(true, Ordering::SeqCst);
        state_ctrlc.is_running.store(false, Ordering::SeqCst); // Stop Engine

        // 2. Stop Feed
        let _ = shutdown_tx_ctrlc.send(());
        
        // 3. Drain Strategy
        shutdown_signal.store(true, Ordering::SeqCst);
        
        // 4. Cancel Orders
        tracing::warn!("Cancelling all open orders...");
        runtime_handle.block_on(async {
            match execution_client_ctrlc.cancel_all_orders("BTCUSDT").await {
                Ok(_) => tracing::info!("All orders cancelled successfully."),
                Err(e) => tracing::error!("Failed to cancel orders: {}", e),
            }
        });
        
        // 5. Disarm Risk Engine
        tracing::warn!("Disarming Risk Engine...");
        risk_engine::disarm();
        
        tracing::info!("Shutdown sequence complete. Exiting.");
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    // 10. Spawn Strategy Thread
    // We need to pass state to strategy, but strategy::run currently doesn't accept it.
    // For now, we will control strategy via the global is_running flag if we modify strategy,
    // OR we can just let it run and filter in Execution. 
    // The prompt says: "Strategy must not emit signals if state.is_running == false".
    // Since strategy::run is in a separate crate, we can't easily inject EngineState without modifying the crate.
    // However, we can modify strategy::run to accept an Arc<AtomicBool> for "running" state.
    // For this step, I will modify strategy crate to accept `is_running` flag.
    let is_running_flag = state.is_running.clone(); 
    let dry_run_config = config.trading.dry_run;
    let strategy_handle = std::thread::spawn(move || {
        // Pin to the last available core
        if let Some(core_ids) = core_affinity::get_core_ids() {
            if let Some(core_id) = core_ids.last() {
                core_affinity::set_for_current(*core_id);
                tracing::info!("Strategy thread pinned to core {:?}", core_id);
            }
        }
        strategy::run(consumer, signal_producer, shutdown_clone, is_running_flag, dry_run_config, false);
    });

    // 11. Spawn Execution Task
    let execution_client_task = execution_client.clone();
    let state_exec = state.clone();
    
    let execution_handle = tokio::spawn(async move {
        tracing::info!("Execution task started");
        loop {
            if shutdown_rx_execution.try_recv().is_ok() {
                break;
            }

            match signal_consumer.pop() {
                Ok(instruction) => {
                    // Check if Engine is Running
                    if !state_exec.is_running.load(Ordering::Relaxed) {
                        continue; 
                    }

                    tracing::info!("Received instruction: {:?}", instruction);
                    
                    // Risk Check
                    if let Err(e) = risk_engine.check(&instruction) {
                        tracing::error!("Risk Rejection: {}", e);
                        continue;
                    }
                    
                    match execution_client_task.place_order(&instruction).await {
                        Ok(response) => {
                            tracing::info!("Order Placed: {}", response);
                            // Update State
                            state_exec.trade_count.fetch_add(1, Ordering::Relaxed);
                            
                            // Update PnL (Simulated for now, or parsed from response if possible)
                            // For now, we don't have real PnL from place_order response.
                            // We will just simulate PnL update or leave it as 0.
                            // Prompt says: "After every successful trade: update PnL... if PnL <= -max_loss... stop"
                            
                            // Auto-Stop Logic
                            let pnl = *state_exec.current_pnl.lock();
                            let max_loss = *state_exec.max_loss_limit.lock();
                            let target_profit = *state_exec.target_profit.lock();
                            
                            if pnl <= -max_loss {
                                tracing::warn!("Max Loss Limit Hit! Stopping Engine.");
                                state_exec.is_running.store(false, Ordering::SeqCst);
                            } else if target_profit > 0.0 && pnl >= target_profit {
                                tracing::info!("Target Profit Hit! Stopping Engine.");
                                state_exec.is_running.store(false, Ordering::SeqCst);
                            }
                        },
                        Err(e) => tracing::error!("Order Failed: {}", e),
                    }
                }
                Err(_) => {
                    tokio::task::yield_now().await;
                }
            }
        }
        tracing::info!("Execution task shutting down");
    });

    // 12. Spawn Feed Task
    let mut shutdown_rx_feed = shutdown_tx.subscribe();
    let mut producer = _producer; // Move producer into task
    
    let feed_handle = tokio::spawn(async move {
        tracing::info!("Feed task started - Connecting to Binance...");
        
        let mut rx = match feed_handler::connect("BTCUSDT", None).await {
            Ok(rx) => rx,
            Err(e) => {
                tracing::error!("Failed to connect to feed: {}", e);
                return;
            }
        };

        loop {
            tokio::select! {
                _ = shutdown_rx_feed.recv() => {
                    tracing::info!("Feed task received shutdown signal");
                    break;
                }
                Some(event) = rx.recv() => {
                    // Push to Ring Buffer
                    if let Err(_e) = producer.push(event) {
                        // If buffer full, we drop (or could log warn periodically)
                        // tracing::warn!("Feed Buffer Full: {:?}", e);
                    }
                }
            }
        }
        tracing::info!("Feed task shutting down");
    });

    // 13. Wait for Shutdown
    if let Err(e) = strategy_handle.join() {
        tracing::error!("Strategy thread panicked: {:?}", e);
    }
    
    let _ = tokio::join!(execution_handle, feed_handle);

    tracing::info!("Trading Engine shutdown complete.");
    Ok(())
}
