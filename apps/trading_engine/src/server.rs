use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post, get_service},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, net::SocketAddr};
use tower_http::services::ServeDir;
use tower_http::cors::CorsLayer;
use crate::state::EngineState;
use std::sync::atomic::Ordering;

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    trade_count: usize,
    pnl: f64,
    max_loss_limit: f64,
    target_profit: f64,
}

#[derive(Deserialize)]
struct ControlRequest {
    command: String,
}

#[derive(Deserialize)]
struct ConfigRequest {
    max_loss: f64,
    target_profit: f64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub async fn run(state: Arc<EngineState>) {
    // Serve static files from "dashboard" directory
    let serve_dir = ServeDir::new("dashboard");

    let app = Router::new()
        .route("/api/status", get(get_status))
        .route("/api/control", post(control_engine))
        .route("/api/config", post(update_config))
        .nest_service("/dashboard", serve_dir.clone())
        .route("/", get_service(serve_dir)) // Redirect root to dashboard
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Web Dashboard listening on http://{}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_status(State(state): State<Arc<EngineState>>) -> impl IntoResponse {
    let pnl = *state.current_pnl.lock();
    let max_loss = *state.max_loss_limit.lock();
    let target_profit = *state.target_profit.lock();

    Json(StatusResponse {
        running: state.is_running.load(Ordering::Relaxed),
        trade_count: state.trade_count.load(Ordering::Relaxed),
        pnl,
        max_loss_limit: max_loss,
        target_profit,
    })
}

async fn control_engine(
    State(state): State<Arc<EngineState>>,
    Json(payload): Json<ControlRequest>,
) -> impl IntoResponse {
    if state.shutting_down.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse { error: "Engine is shutting down".to_string() }),
        ).into_response();
    }

    match payload.command.as_str() {
        "START" => {
            state.is_running.store(true, Ordering::SeqCst);
            risk_engine::arm(); // Arm the system
            tracing::info!("API Command: START - System Armed");
            (StatusCode::OK, Json(serde_json::json!({"status": "started"}))).into_response()
        }
        "STOP" => {
            state.is_running.store(false, Ordering::SeqCst);
            risk_engine::disarm(); // Disarm the system
            tracing::info!("API Command: STOP - System Disarmed");
            (StatusCode::OK, Json(serde_json::json!({"status": "stopped"}))).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: "Invalid command".to_string() }),
        ).into_response(),
    }
}

async fn update_config(
    State(state): State<Arc<EngineState>>,
    Json(payload): Json<ConfigRequest>,
) -> impl IntoResponse {
    if state.shutting_down.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse { error: "Engine is shutting down".to_string() }),
        ).into_response();
    }

    *state.max_loss_limit.lock() = payload.max_loss;
    *state.target_profit.lock() = payload.target_profit;

    tracing::info!(
        "API Config Update: Max Loss={}, Target Profit={}",
        payload.max_loss,
        payload.target_profit
    );

    (StatusCode::OK, Json(serde_json::json!({"status": "updated"}))).into_response()
}
