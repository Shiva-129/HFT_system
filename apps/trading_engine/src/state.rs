use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize},
    Arc,
};

pub struct EngineState {
    /// Global Start/Stop switch.
    pub is_running: Arc<AtomicBool>,
    /// Exit guard.
    pub shutting_down: AtomicBool,
    /// Number of executed trades.
    pub trade_count: AtomicUsize,

    /// Real-time P&L.
    pub current_pnl: Mutex<f64>,
    /// Hard stop-loss limit.
    pub max_loss_limit: Mutex<f64>,
    /// Take-profit limit.
    pub target_profit: Mutex<f64>,

    pub initial_balance: Mutex<f64>,
    pub available_balance: Mutex<f64>,

    // --- Telemetry ---
    pub last_tick_timestamp: AtomicU64, // Epoch ms
    pub last_order_rtt_ns: AtomicU64,
    pub current_position: Mutex<f64>,
    pub avg_entry_price: Mutex<f64>,
    pub last_price: Mutex<f64>,

    // History (Capped)
    pub pnl_history: Mutex<VecDeque<(u64, f64)>>, // (ts_ms, pnl)
    pub recent_logs: Mutex<VecDeque<String>>,
    pub active_strategy: Arc<Mutex<String>>,

    // Speed Meter
    pub ticks_counter: AtomicUsize,
    pub cycles_counter: AtomicUsize,
    pub current_tps: AtomicUsize,
    pub current_cps: AtomicUsize,
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            shutting_down: AtomicBool::new(false),
            trade_count: AtomicUsize::new(0),
            current_pnl: Mutex::new(0.0),
            max_loss_limit: Mutex::new(0.0),
            target_profit: Mutex::new(0.0),
            initial_balance: Mutex::new(0.0),
            available_balance: Mutex::new(0.0),

            last_tick_timestamp: AtomicU64::new(0),
            last_order_rtt_ns: AtomicU64::new(0),
            current_position: Mutex::new(0.0),
            avg_entry_price: Mutex::new(0.0),
            last_price: Mutex::new(0.0),

            pnl_history: Mutex::new(VecDeque::with_capacity(5000)),
            recent_logs: Mutex::new(VecDeque::with_capacity(200)),
            active_strategy: Arc::new(Mutex::new("PING_PONG".to_string())),

            ticks_counter: AtomicUsize::new(0),
            cycles_counter: AtomicUsize::new(0),
            current_tps: AtomicUsize::new(0),
            current_cps: AtomicUsize::new(0),
        }
    }

    pub fn add_log(&self, msg: String) {
        let mut logs = self.recent_logs.lock();
        if logs.len() >= 200 {
            logs.pop_front();
        }
        logs.push_back(msg);
    }

    pub fn update_from_trade(&self, qty: f64, price: f64, fee: f64) -> f64 {
        let mut pos = self.current_position.lock();
        let mut avg_entry = self.avg_entry_price.lock();
        let mut realized_pnl = 0.0;

        let old_pos = *pos;
        let new_pos = old_pos + qty;

        // Check if reducing position (signs opposite)
        if (old_pos > 0.0 && qty < 0.0) || (old_pos < 0.0 && qty > 0.0) {
            // Closing some amount
            let closing_qty = if old_pos.abs() < qty.abs() {
                old_pos.abs() // Full close + flip
            } else {
                qty.abs() // Partial close
            };

            if old_pos > 0.0 {
                // Long closing
                realized_pnl = (price - *avg_entry) * closing_qty;
            } else {
                // Short closing
                realized_pnl = (*avg_entry - price) * closing_qty;
            }
        }

        // Deduct Fee (Always)
        realized_pnl -= fee;

        // Update Avg Entry Price
        if new_pos == 0.0 {
            *avg_entry = 0.0;
        } else if (old_pos >= 0.0 && qty > 0.0) || (old_pos <= 0.0 && qty < 0.0) {
            // Increasing position (or starting new)
            let total_cost = (old_pos.abs() * *avg_entry) + (qty.abs() * price);
            *avg_entry = total_cost / new_pos.abs();
        } else if (old_pos > 0.0 && new_pos < 0.0) || (old_pos < 0.0 && new_pos > 0.0) {
            // Flipped position
            // The remaining qty is new position at new price
            *avg_entry = price;
        }
        // If reducing but not flipping, avg_entry stays the same.

        *pos = new_pos;

        // Update Global PnL
        // We update PnL if there is realized PnL OR if there is a fee (even on open)
        if realized_pnl != 0.0 || fee > 0.0 {
            let mut pnl_lock = self.current_pnl.lock();
            *pnl_lock += realized_pnl;

            // Add to history
            let mut history = self.pnl_history.lock();
            if history.len() >= 5000 {
                history.pop_front();
            }
            history.push_back((common::now_nanos() / 1_000_000, *pnl_lock));
        }

        realized_pnl
    }
}
