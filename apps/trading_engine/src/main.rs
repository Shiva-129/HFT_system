mod config;
mod db;
mod server;
mod state;

use crate::state::EngineState;
use execution::ExecutionClient;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

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
    tracing::info!(
        "Config loaded: Network={}, DryRun={}",
        config.network.name,
        config.trading.dry_run
    );

    // 3. Initialize Shared State
    let state = Arc::new(EngineState::new());
    // Initialize limits from config
    *state.max_loss_limit.lock() = config.risk.max_drawdown; // Using max_drawdown as initial max_loss
                                                             // target_profit is 0.0 by default, can be set via API

    // 4. Initialize Database
    let db = db::TradeStorage::new("trading.db").await?;
    tracing::info!("Database connected");

    // 5. Spawn Web Server
    let server_state = state.clone();
    let server_db = db.clone();
    tokio::spawn(async move {
        server::run(server_state, server_db).await;
    });

    // 6. Spawn Speed Meter Task
    let speed_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let ticks = speed_state.ticks_counter.swap(0, Ordering::Relaxed);
            let cycles = speed_state.cycles_counter.swap(0, Ordering::Relaxed);
            speed_state.current_tps.store(ticks, Ordering::Relaxed);
            speed_state.current_cps.store(cycles, Ordering::Relaxed);
        }
    });

    // 7. Initialize Execution Client
    let api_key = config.trading.api_key.clone().unwrap_or_default();
    let secret_key = config.trading.secret_key.clone().unwrap_or_default();

    if config.trading.enabled && (api_key.is_empty() || secret_key.is_empty()) {
        tracing::error!("Trading enabled but API keys missing!");
        std::process::exit(1);
    }

    let execution_client = Arc::new(ExecutionClient::new(
        api_key,
        secret_key,
        config.network.rest_url.clone(),
    ));

    // 8. Initialize Risk Engine
    let mut risk_engine =
        risk_engine::RiskEngine::new(config.risk.max_order_size, config.risk.max_drawdown);

    // 9. Position Sync
    tracing::info!("Syncing positions...");
    match execution_client.sync_positions().await {
        Ok(positions) => {
            tracing::info!("Position sync OK: {} positions found", positions.len());
            for p in positions {
                if p.symbol == "BTCUSDT" {
                    *state.current_position.lock() = p.position_amt.parse::<f64>().unwrap_or(0.0);
                    tracing::info!("  Active Position: {} = {}", p.symbol, p.position_amt);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to sync positions: {}", e);
            if e.to_string().contains("AUTH_ERROR") && config.trading.enabled {
                tracing::error!("CRITICAL: Invalid API Keys. Exiting.");
                std::process::exit(1);
            }
        }
    }

    // 9b. Balance Sync
    tracing::info!("Syncing balance...");
    match execution_client.get_account_balance().await {
        Ok(balances) => {
            for b in balances {
                if b.asset == "USDT" {
                    let balance = b.balance.parse::<f64>().unwrap_or(0.0);
                    *state.initial_balance.lock() = balance;
                    tracing::info!("  Initial Balance: USDT = {}", balance);
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to sync balance: {}", e);
        }
    }

    // 10. Setup Ring Buffers
    let (producer, consumer) = rtrb::RingBuffer::<common::MarketEvent>::new(4096);
    let (signal_producer, mut signal_consumer) =
        rtrb::RingBuffer::<common::TradeInstruction>::new(4096);

    // 11. Shutdown Signals
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let mut shutdown_rx_execution = shutdown_tx.subscribe();
    let shutdown_tx_ctrlc = shutdown_tx.clone();
    let shutdown_signal = shutdown.clone();

    // 12. Graceful Shutdown Handler
    let cleanup_done = Arc::new(AtomicBool::new(false));
    let state_ctrlc = state.clone();
    let db_ctrlc = db.clone();
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
            // TODO: Call cancel_all_orders
        });

        // 5. Disarm Risk Engine
        tracing::warn!("Disarming Risk Engine...");
        risk_engine::disarm();

        // 6. Flush DB
        tracing::warn!("Flushing Database...");
        runtime_handle.block_on(async {
            db_ctrlc.flush().await;
        });

        tracing::info!("Shutdown sequence complete. Exiting.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    // 13. Spawn Strategy Thread
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
        strategy::run(
            consumer,
            signal_producer,
            shutdown_clone,
            is_running_flag,
            dry_run_config,
            false,
        );
    });

    // 14. Spawn Execution Task
    let execution_client_task = execution_client.clone();
    let state_exec = state.clone();
    let db_exec = db.clone();

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
                        state_exec.add_log(format!("Risk Reject: {}", e));
                        continue;
                    }

                    // Measure RTT
                    let start = std::time::Instant::now();

                    match execution_client_task.place_order(&instruction).await {
                        Ok(response) => {
                            let rtt = start.elapsed().as_nanos() as u64;
                            state_exec.last_order_rtt_ns.store(rtt, Ordering::Relaxed);

                            tracing::info!("Order Placed: {}", response);
                            state_exec.trade_count.fetch_add(1, Ordering::Relaxed);
                            state_exec.add_log(format!(
                                "Order Placed: {:?} {} @ {}",
                                instruction.side, instruction.quantity, instruction.price
                            ));

                            // Calculate PnL
                            let signed_qty = match instruction.side {
                                common::Side::Buy => instruction.quantity,
                                common::Side::Sell => -instruction.quantity,
                            };
                            let realized_pnl =
                                state_exec.update_from_trade(signed_qty, instruction.price);

                            // DB Insert
                            db_exec
                                .insert_trade(crate::db::TradeRecord {
                                    exchange_ts_ms: common::now_nanos() as i64 / 1_000_000, // Approx
                                    monotonic_ns: common::now_nanos(),
                                    symbol: instruction.symbol.to_string(),
                                    side: format!("{:?}", instruction.side),
                                    price: instruction.price,
                                    quantity: instruction.quantity,
                                    pnl: realized_pnl,
                                    strategy: "PING_PONG".to_string(),
                                    order_id: None, // Parse from response
                                    exec_id: None,
                                    fee: None,
                                    fee_currency: None,
                                    raw: Some(response),
                                })
                                .await;

                            // Auto-Stop Logic
                            let pnl = *state_exec.current_pnl.lock();
                            let max_loss = *state_exec.max_loss_limit.lock();
                            let target_profit = *state_exec.target_profit.lock();

                            if pnl <= -max_loss {
                                tracing::warn!("Max Loss Limit Hit! Stopping Engine.");
                                state_exec.is_running.store(false, Ordering::SeqCst);
                            }
                            if target_profit > 0.0 && pnl >= target_profit {
                                tracing::info!("Target Profit Hit! Stopping Engine.");
                                state_exec.is_running.store(false, Ordering::SeqCst);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Order Failed: {}", e);
                            state_exec.add_log(format!("Order Failed: {}", e));
                        }
                    }
                }
                Err(_) => {
                    tokio::task::yield_now().await;
                }
            }
        }
        tracing::info!("Execution task shutting down");
    });

    // 15. Spawn Feed Task
    let mut shutdown_rx_feed = shutdown_tx.subscribe();
    let mut producer = producer; // Move producer into task
    let state_feed = state.clone();

    let feed_handle = tokio::spawn(async move {
        tracing::info!("Feed task started - Connecting to Binance...");

        let mut rx = match feed_handler::connect("BTCUSDT", None).await {
            Ok(rx) => rx,
            Err(e) => {
                tracing::error!("Failed to connect to feed: {}", e);
                return;
            }
        };

        tracing::info!("Connected to Binance for btcusdt");

        loop {
            tokio::select! {
                _ = shutdown_rx_feed.recv() => {
                    break;
                }
                Some(event) = rx.recv() => {
                    // Update Heartbeat
                    state_feed.last_tick_timestamp.store(event.exchange_timestamp as u64, Ordering::Relaxed);
                    state_feed.ticks_counter.fetch_add(1, Ordering::Relaxed);

                    // Push to RingBuffer
                    if let Err(_e) = producer.push(event) {
                        // tracing::warn!("Ring buffer full, dropping tick");
                    }
                }
            }
        }
        tracing::info!("Feed task shutting down");
    });

    // 16. Wait for Strategy Thread
    if let Err(e) = strategy_handle.join() {
        tracing::error!("Strategy thread panicked: {:?}", e);
    }

    let _ = tokio::join!(execution_handle, feed_handle);

    tracing::info!("Trading Engine shutdown complete.");
    Ok(())
}
