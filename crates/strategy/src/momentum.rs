use common::{MarketEvent, OrderType, Side, TradeInstruction};
use std::collections::VecDeque;

pub struct MomentumStrategy {
    price_history: VecDeque<f64>,
    window_size: usize,
    threshold: f64,
    position: i32, // 0 = Flat, 1 = Long, -1 = Short
    last_signal_time: u64,
    _fee_maker: f64,
    fee_taker: f64,
}

impl MomentumStrategy {
    pub fn new(window_size: usize, threshold: f64, fee_maker: f64, fee_taker: f64) -> Self {
        Self {
            price_history: VecDeque::with_capacity(window_size),
            window_size,
            threshold,
            position: 0,
            last_signal_time: 0,
            _fee_maker: fee_maker,
            fee_taker,
        }
    }
}

use crate::Strategy;

impl Strategy for MomentumStrategy {
    fn process_event(&mut self, event: &MarketEvent) -> Option<TradeInstruction> {
        // Step 1: Add price to history
        if self.price_history.len() >= self.window_size {
            self.price_history.pop_front();
        }
        self.price_history.push_back(event.price);

        // Step 2: Calculate velocity (only if history is full)
        if self.price_history.len() < self.window_size {
            return None;
        }

        let current_price = event.price;
        let oldest_price = *self.price_history.front().unwrap();
        let velocity = current_price - oldest_price;
        let now = common::now_nanos();

        // Step 5 (Safety): Cooldown check (1 second = 1_000_000_000 ns)
        if now - self.last_signal_time < 1_000_000_000 {
            return None;
        }

        let mut instruction = None;

        // Calculate total round-trip fee (entry + exit) as a percentage of price
        // Assuming Taker for both entry and exit for simplicity/safety
        let fee_cost = current_price * (self.fee_taker * 2.0);
        let effective_threshold = self.threshold + fee_cost;

        // Debug Logging (Throttle to every 100 ticks approx)
        if now % 100 == 0 {
            tracing::info!(
                "Momentum Debug: Velocity={:.2}, Threshold={:.2} (Base={:.2} + Fee={:.2})",
                velocity,
                effective_threshold,
                self.threshold,
                fee_cost
            );
        }

        // Step 3 (Entry Logic) & Step 4 (Exit Logic)
        if self.position == 0 {
            // Entry
            if velocity > effective_threshold {
                // Buy Signal
                tracing::info!(
                    "Momentum BUY: Velocity {:.2} > Threshold {:.2} (Base {:.2} + Fee {:.2})",
                    velocity,
                    effective_threshold,
                    self.threshold,
                    fee_cost
                );
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: Side::Buy,
                    price: event.price,
                    order_type: OrderType::Market,
                    quantity: 0.01, // Fixed quantity for now
                    timestamp: now,
                    dry_run: false, // Default to false or pass in config if needed
                });
                self.position = 1;
                self.last_signal_time = now;
            } else if velocity < -effective_threshold {
                // Sell Signal
                tracing::info!(
                    "Momentum SELL: Velocity {:.2} < -Threshold {:.2} (Base {:.2} + Fee {:.2})",
                    velocity,
                    effective_threshold,
                    self.threshold,
                    fee_cost
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
        } else if self.position == 1 {
            // Exit Long
            if velocity < 0.0 {
                tracing::info!("Momentum CLOSE LONG: Velocity {:.2} < 0", velocity);
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: Side::Sell, // Close Long by Selling
                    price: event.price,
                    order_type: OrderType::Market,
                    quantity: 0.01,
                    timestamp: now,
                    dry_run: false,
                });
                self.position = 0;
                self.last_signal_time = now;
            }
        } else if self.position == -1 {
            // Exit Short
            if velocity > 0.0 {
                tracing::info!("Momentum CLOSE SHORT: Velocity {:.2} > 0", velocity);
                instruction = Some(TradeInstruction {
                    symbol: event.symbol.clone(),
                    side: Side::Buy, // Close Short by Buying
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
