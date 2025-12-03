use crate::db::TradeStorage;
use crate::state::EngineState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        IntoResponse, Json,
    },
    routing::{get, get_service, post},
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    trade_count: usize,
    pnl: f64,
    max_loss_limit: f64,
    target_profit: f64,
    initial_balance: f64,
    available_balance: f64,
    current_position: f64,
    last_tick_ts: u64,
    last_order_rtt_ns: u64,
    active_strategy: String,
    tps: usize,
    cps: usize,
}

// ... (ControlRequest, ConfigRequest, etc. skipped)

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let engine = &state.engine;
    let running = engine.is_running.load(Ordering::Relaxed);
    let trade_count = engine.trade_count.load(Ordering::Relaxed);
    let pnl = *engine.current_pnl.lock();
    let max_loss_limit = *engine.max_loss_limit.lock();
    let target_profit = *engine.target_profit.lock();
    let initial_balance = *engine.initial_balance.lock();
    let available_balance = *engine.available_balance.lock();
    let current_position = *engine.current_position.lock();
    let last_tick_ts = engine.last_tick_timestamp.load(Ordering::Relaxed);
    let last_order_rtt_ns = engine.last_order_rtt_ns.load(Ordering::Relaxed);
    let active_strategy = engine.active_strategy.lock().clone();
    let tps = engine.current_tps.load(Ordering::Relaxed);
    let cps = engine.current_cps.load(Ordering::Relaxed);

    Json(StatusResponse {
        running,
        trade_count,
        pnl,
        max_loss_limit,
        target_profit,
        initial_balance,
        available_balance,
        current_position,
        last_tick_ts,
        last_order_rtt_ns,
        active_strategy,
        tps,
        cps,
    })
}

#[derive(Deserialize)]
struct ControlRequest {
    command: String,
    confirm: Option<bool>,
}

#[derive(Deserialize)]
struct ConfigRequest {
    max_loss: f64,
    target_profit: f64,
}

#[derive(Deserialize)]
struct StrategyRequest {
    strategy: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// Wrapper for shared state
#[derive(Clone)]
pub struct AppState {
    engine: Arc<EngineState>,
    db: TradeStorage,
}

pub async fn run(state: Arc<EngineState>, db: TradeStorage) {
    let serve_dir = ServeDir::new("dashboard");
    let app_state = AppState { engine: state, db };

    let app = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/control", post(control_engine))
        .route("/api/config", post(update_config))
        .route("/api/strategy", post(set_strategy))
        .route("/api/strategies", get(get_strategies))
        .route("/api/history", get(get_history).delete(clear_history))
        .route("/api/pnl_series", get(get_pnl_series))
        .route("/api/logs", get(get_logs))
        .route("/api/sse", get(sse_handler))
        .nest_service("/dashboard", serve_dir.clone())
        .route("/", get_service(serve_dir))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Web Dashboard listening on http://{}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn control_engine(
    State(state): State<AppState>,
    Json(payload): Json<ControlRequest>,
) -> impl IntoResponse {
    if state.engine.shutting_down.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Engine is shutting down".to_string(),
            }),
        )
            .into_response();
    }

    match payload.command.as_str() {
        "START" => {
            state.engine.is_running.store(true, Ordering::SeqCst);
            risk_engine::arm();
            state.engine.add_log("System Started".to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "started"})),
            )
                .into_response()
        }
        "STOP" => {
            state.engine.is_running.store(false, Ordering::SeqCst);
            risk_engine::disarm();
            state.engine.add_log("System Stopped".to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "stopped"})),
            )
                .into_response()
        }
        "FLATTEN" => {
            // Safety: Require confirmation
            if payload.confirm != Some(true) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Confirmation required for FLATTEN".to_string(),
                    }),
                )
                    .into_response();
            }

            // Enqueue Flatten Job (Mock implementation for now)
            // In a real system, this would push to a high-priority channel consumed by Execution
            state
                .engine
                .add_log("FLATTEN command received. Queuing emergency close.".to_string());
            tracing::warn!("FLATTEN COMMAND RECEIVED");

            // TODO: Implement actual flatten logic via ExecutionClient

            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({"status": "flatten_queued"})),
            )
                .into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid command".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn update_config(
    State(state): State<AppState>,
    Json(payload): Json<ConfigRequest>,
) -> impl IntoResponse {
    *state.engine.max_loss_limit.lock() = payload.max_loss;
    *state.engine.target_profit.lock() = payload.target_profit;
    state.engine.add_log(format!(
        "Config Updated: MaxLoss={}, Target={}",
        payload.max_loss, payload.target_profit
    ));
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "updated"})),
    )
        .into_response()
}

async fn set_strategy(
    State(state): State<AppState>,
    Json(payload): Json<StrategyRequest>,
) -> impl IntoResponse {
    let current_pos = *state.engine.current_position.lock();
    if current_pos.abs() > 0.000001 {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "Cannot change strategy with open positions".to_string(),
            }),
        )
            .into_response();
    }

    *state.engine.active_strategy.lock() = payload.strategy.clone();
    state
        .engine
        .add_log(format!("Strategy changed to {}", payload.strategy));
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "strategy_updated"})),
    )
        .into_response()
}

async fn get_strategies() -> impl IntoResponse {
    Json(strategy::AVAILABLE_STRATEGIES).into_response()
}

async fn get_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    match state.db.get_recent_trades(limit).await {
        Ok(trades) => Json(trades).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn clear_history(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.clear_trades().await {
        Ok(_) => {
            state
                .engine
                .add_log("Trade history cleared by user".to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "cleared"})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct PnlQuery {
    mode: Option<String>,
}

async fn get_pnl_series(
    State(state): State<AppState>,
    Query(params): Query<PnlQuery>,
) -> impl IntoResponse {
    if let Some(mode) = &params.mode {
        if mode == "historical" {
            match state.db.get_all_trades_asc().await {
                Ok(trades) => {
                    let mut history = std::collections::VecDeque::new();
                    let mut cumulative_pnl = 0.0;

                    for trade in trades {
                        cumulative_pnl += trade.pnl;
                        history.push_back((trade.exchange_ts_ms as u64, cumulative_pnl));
                    }
                    return Json(history).into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: e.to_string(),
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    let history = state.engine.pnl_history.lock().clone();
    Json(history).into_response()
}

async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let logs = state.engine.recent_logs.lock().clone();
    Json(logs).into_response()
}

// SSE Handler
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut interval = tokio::time::interval(Duration::from_millis(500)); // 2Hz updates
        loop {
            interval.tick().await;

            let pnl = *state.engine.current_pnl.lock();
            let last_tick = state.engine.last_tick_timestamp.load(Ordering::Relaxed);
            let tps = state.engine.current_tps.load(Ordering::Relaxed);
            let trade_count = state.engine.trade_count.load(Ordering::Relaxed);
            let position = *state.engine.current_position.lock();
            let avg_entry = *state.engine.avg_entry_price.lock();
            let last_price = *state.engine.last_price.lock();
            let available_balance = *state.engine.available_balance.lock();

            let unrealized_pnl = (last_price - avg_entry) * position;

            let data = serde_json::json!({
                "pnl": pnl,
                "unrealized_pnl": unrealized_pnl,
                "last_tick": last_tick,
                "tps": tps,
                "trade_count": trade_count,
                "position": position,
                "available_balance": available_balance,
                "ts": common::now_nanos() / 1_000_000 // ms
            });

            yield Ok(Event::default().data(data.to_string()));
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}
