use serde::Deserialize;
use common::{MarketEvent, EngineError};
use once_cell::sync::Lazy;
use std::time::Instant;

static MONOTONIC_START: Lazy<Instant> = Lazy::new(Instant::now);
#[allow(non_snake_case)]
#[derive(Deserialize)]
pub struct BinanceAggTrade {
    s: String,
    p: String,
    q: String,
    T: i64,
}

impl TryFrom<BinanceAggTrade> for MarketEvent {
    type Error = EngineError;

    fn try_from(trade: BinanceAggTrade) -> Result<Self, Self::Error> {
        let price = trade.p.parse::<f64>()
            .map_err(|e| EngineError::ParseError(format!("Invalid price: {}", e)))?;
        let quantity = trade.q.parse::<f64>()
            .map_err(|e| EngineError::ParseError(format!("Invalid quantity: {}", e)))?;

        Ok(MarketEvent {
            symbol: trade.s.to_ascii_uppercase(),
            price,
            quantity,
            exchange_timestamp: trade.T,
            received_timestamp: MONOTONIC_START.elapsed().as_nanos() as u64,
        })
    }
}

pub fn parse_trade(value: &str) -> Result<MarketEvent, EngineError> {
    let trade: BinanceAggTrade = serde_json::from_str(value)
        .map_err(|e| EngineError::ParseError(e.to_string()))?;
    
    trade.try_into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_binance_trade() {
        let raw = r#"{"e":"aggTrade","E":123456789,"s":"BTCUSDT","a":123,"p":"50000.0","q":"1.0","f":100,"l":105,"T":1630000000000,"m":true,"M":true}"#;
        let event = parse_trade(raw).expect("Failed to parse");
        
        assert_eq!(event.symbol, "BTCUSDT");
        assert_eq!(event.price, 50000.0);
        assert_eq!(event.quantity, 1.0);
        assert_eq!(event.exchange_timestamp, 1630000000000);
        assert!(event.received_timestamp > 0);
    }
}
