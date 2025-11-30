use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize}};
use parking_lot::Mutex;

pub struct EngineState {
    /// Global Start/Stop switch. If false, Strategy and Execution should pause.
    pub is_running: Arc<AtomicBool>,
    /// Exit guard. If true, API should reject control commands.
    pub shutting_down: AtomicBool,
    /// Number of executed trades.
    pub trade_count: AtomicUsize,
    /// Real-time P&L.
    pub current_pnl: Mutex<f64>,
    /// Hard stop-loss limit.
    pub max_loss_limit: Mutex<f64>,
    /// Take-profit limit.
    pub target_profit: Mutex<f64>,
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)), // Start stopped
            shutting_down: AtomicBool::new(false),
            trade_count: AtomicUsize::new(0),
            current_pnl: Mutex::new(0.0),
            max_loss_limit: Mutex::new(0.0), // Will be updated from config
            target_profit: Mutex::new(0.0), // Will be updated from config
        }
    }
}
