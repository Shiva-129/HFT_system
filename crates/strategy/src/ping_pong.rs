use common::{MarketEvent, OrderType, Side, TradeInstruction};
use std::time::{Duration, Instant};

pub struct PingPongStrategy {
    last_trade_time: Instant,
    next_side: Side,
    dry_run: bool,
}

impl PingPongStrategy {
    pub fn new(dry_run: bool) -> Self {
        Self {
            last_trade_time: Instant::now() - Duration::from_secs(20),
            next_side: Side::Buy,
            dry_run,
        }
    }

    pub fn process_event(
        &mut self,
        event: &MarketEvent,
        disable_throttle: bool,
    ) -> Option<TradeInstruction> {
        let throttle_passed =
            disable_throttle || self.last_trade_time.elapsed() > Duration::from_secs(10);

        if event.price > 50_000.0 && throttle_passed {
            let instr = TradeInstruction {
                symbol: event.symbol.clone(),
                side: self.next_side,
                order_type: OrderType::Market,
                price: event.price,
                quantity: 0.01,
                timestamp: common::now_nanos(),
                dry_run: self.dry_run,
            };

            self.last_trade_time = Instant::now();
            
            // Toggle side
            self.next_side = match self.next_side {
                Side::Buy => Side::Sell,
                Side::Sell => Side::Buy,
            };
            
            tracing::info!("Strategy: Switched next side to {:?}", self.next_side);

            Some(instr)
        } else {
            None
        }
    }
}
