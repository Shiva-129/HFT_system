use common::{MarketEvent, OrderType, Side, TradeInstruction};
use std::collections::VecDeque;

pub struct LiquidationStrategy {
    price_history: VecDeque<f64>,
    volume_history: VecDeque<f64>,
    avg_volume: f64,
    position: i32, // 0 = Flat, 1 = Long, -1 = Short
    last_signal_time: u64,
    price_threshold: f64,
    volume_multiplier: f64,
    window_size: usize,
}

impl LiquidationStrategy {
    pub fn new(price_threshold: f64, volume_multiplier: f64) -> Self {
        let window_size = 50;
        Self {
            price_history: VecDeque::with_capacity(window_size),
            volume_history: VecDeque::with_capacity(window_size),
            avg_volume: 0.0,
            position: 0,
            last_signal_time: 0,
            price_threshold,
            volume_multiplier,
            window_size,
        }
    }
}

use crate::Strategy;

impl Strategy for LiquidationStrategy {
    fn process_event(&mut self, event: &MarketEvent) -> Option<TradeInstruction> {
        // Step 1: Track Data
        if self.price_history.len() >= self.window_size {
            self.price_history.pop_front();
        }
        self.price_history.push_back(event.price);

        if self.volume_history.len() >= self.window_size {
            self.volume_history.pop_front();
        }
        self.volume_history.push_back(event.quantity);

        // Update rolling average volume
        if !self.volume_history.is_empty() {
            self.avg_volume =
                self.volume_history.iter().sum::<f64>() / self.volume_history.len() as f64;
        }

        // Need full history before trading
        if self.price_history.len() < self.window_size {
            return None;
        }

        let now = common::now_nanos();

        // Cooldown check (1 second)
        if now - self.last_signal_time < 1_000_000_000 {
            return None;
        }

        // Step 2: Detect Cascade
        let current_price = event.price;
        let price_50_ticks_ago = *self.price_history.front().unwrap();
        let price_velocity = current_price - price_50_ticks_ago;

        // Current volume burst (last 5 ticks)
        let burst_window = 5.min(self.volume_history.len());
        let current_volume: f64 = self.volume_history.iter().rev().take(burst_window).sum();

        let mut instruction = None;

        // Debug Logging (every ~100 ticks)
        if now % 100 == 0 {
            let recent_avg_volume = current_volume / burst_window as f64;
            tracing::info!(
                "LIQUIDATION Debug: Velocity={:.2}, PriceThreshold={:.2}, RecentVolAvg={:.4}, RollingVolAvg={:.4}, VolMultiplier={:.1}x, Position={}",
                price_velocity, self.price_threshold, recent_avg_volume, self.avg_volume, self.volume_multiplier, self.position
            );
        }

        // Step 3: Trigger (The Vulture)
        if self.position == 0 {
            // LONG Signal: Upward cascade with heavy volume
            if price_velocity > self.price_threshold
                && current_volume > (self.avg_volume * self.volume_multiplier)
            {
                tracing::info!(
                    "LIQUIDATION BUY: Velocity={:.2}, Threshold={:.2}, Volume={:.4}, AvgVol={:.4} ({}x)",
                    price_velocity, self.price_threshold, current_volume, self.avg_volume, self.volume_multiplier
                );
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: Side::Buy,
                    price: event.price,
                    order_type: OrderType::Market,
                    quantity: 0.01,
                    timestamp: now,
                    dry_run: false,
                });
                self.position = 1;
                self.last_signal_time = now;
            }
            // SHORT Signal: Downward cascade with heavy volume
            else if price_velocity < -self.price_threshold
                && current_volume > (self.avg_volume * self.volume_multiplier)
            {
                tracing::info!(
                    "LIQUIDATION SELL: Velocity={:.2}, Threshold={:.2}, Volume={:.4}, AvgVol={:.4} ({}x)",
                    price_velocity, self.price_threshold, current_volume, self.avg_volume, self.volume_multiplier
                );
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: Side::Sell,
                    price: event.price,
                    order_type: OrderType::Market,
                    quantity: 0.01,
                    timestamp: now,
                    dry_run: false,
                });
                self.position = -1;
                self.last_signal_time = now;
            }
        }
        // Step 4: Exit (Mean Reversion)
        else if self.position != 0 {
            // Close when volume drops back below average (panic is over)
            // Compare average of last 5 ticks vs rolling average
            let recent_avg_volume = current_volume / burst_window as f64;

            if recent_avg_volume <= self.avg_volume {
                let exit_side = if self.position == 1 {
                    Side::Sell
                } else {
                    Side::Buy
                };
                tracing::info!(
                    "LIQUIDATION EXIT: Volume normalized (Recent Avg: {:.4} <= Rolling Avg: {:.4}), closing position",
                    recent_avg_volume, self.avg_volume
                );
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: exit_side,
                    price: event.price,
                    order_type: OrderType::Market,
                    quantity: 0.01,
                    timestamp: now,
                    dry_run: false,
                });
                self.position = 0;
                self.last_signal_time = now;
            }
        }

        instruction
    }
}
