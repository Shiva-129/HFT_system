use serde::{Deserialize, Serialize};

/// Side of the order (Buy or Sell)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

/// Type of the order (Limit or Market)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum OrderType {
    Limit,
    Market,
}

/// Represents a market event (e.g., a trade or quote update)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketEvent {
    pub symbol: String,
    pub price: f64,
    pub quantity: f64,
    /// Raw Unix timestamp in milliseconds, as provided by the exchange.
    pub exchange_timestamp: i64,
    /// Local monotonic timestamp in nanoseconds, suitable for latency measurement.
    pub received_timestamp: u64,
}

/// Represents an instruction to execute a trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeInstruction {
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub price: f64,
    pub quantity: f64,
    pub timestamp: u64,
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_event_serialization() {
        let event = MarketEvent {
            symbol: "BTCUSDT".to_string(),
            price: 50000.0,
            quantity: 1.5,
            exchange_timestamp: 1630000000000,
            received_timestamp: 123456789,
        };

        let serialized = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: MarketEvent = serde_json::from_str(&serialized).expect("Failed to deserialize");

        assert_eq!(event, deserialized);
    }
}
