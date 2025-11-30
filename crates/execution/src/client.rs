use crate::signer::BinanceSigner;
use common::{EngineError, TradeInstruction, OrderType};
use reqwest::Client;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicUsize, AtomicU64, Ordering};

pub struct ExecutionClient {
    http_client: Client,
    signer: BinanceSigner,
    base_url: String,
    // Rate Limiting: Max 10 requests per second
    rate_limit_count: AtomicUsize,
    rate_limit_reset: AtomicU64,
}

impl ExecutionClient {
    pub fn new(api_key: String, secret_key: String, base_url: String) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http_client,
            signer: BinanceSigner::new(api_key, secret_key),
            base_url,
            rate_limit_count: AtomicUsize::new(0),
            rate_limit_reset: AtomicU64::new(0),
        }
    }

    /// Helper to format decimals: 8 decimal places, trim trailing zeros and dot.
    fn fmt_decimal(v: f64) -> String {
        let s = format!("{:.8}", v);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }

    /// Checks if we are exceeding the internal rate limit (10 req/sec).
    /// Returns true if allowed, false if blocked.
    fn check_rate_limit(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let reset_time = self.rate_limit_reset.load(Ordering::Relaxed);

        if now > reset_time {
            // New second window
            self.rate_limit_reset.store(now, Ordering::Relaxed);
            self.rate_limit_count.store(1, Ordering::Relaxed);
            true
        } else {
            // Same second window
            let count = self.rate_limit_count.fetch_add(1, Ordering::Relaxed);
            if count < 10 {
                true
            } else {
                false
            }
        }
    }

    /// Place an order. If instruction.dry_run == true, return Ok("DRY_RUN_SUCCESS").
    pub async fn place_order(&self, instruction: &TradeInstruction) -> Result<String, EngineError> {
        if instruction.dry_run {
            return Ok("DRY_RUN_SUCCESS".to_string());
        }

        if !self.check_rate_limit() {
            return Err(EngineError::ExchangeError("Internal Rate Limit Exceeded (10 req/s)".to_string()));
        }

        // 1. Build Canonical Query String
        // Order: symbol, side, type, quantity, timeInForce (if Limit), price (if Limit), recvWindow, timestamp
        let mut query = format!(
            "symbol={}&side={:?}&type={:?}&quantity={}",
            instruction.symbol.to_uppercase(),
            instruction.side,
            instruction.order_type,
            Self::fmt_decimal(instruction.quantity)
        );

        if instruction.order_type == OrderType::Limit {
            query.push_str("&timeInForce=GTC");
            query.push_str(&format!("&price={}", Self::fmt_decimal(instruction.price)));
        }

        // Add recvWindow and timestamp
        let timestamp = chrono::Utc::now().timestamp_millis();
        query.push_str(&format!("&recvWindow=5000&timestamp={}", timestamp));

        // 2. Sign
        let signature = self.signer.sign(&query);
        let signed_body = format!("{}&signature={}", query, signature);

        // 3. Send Request
        let url = format!("{}/fapi/v1/order", self.base_url);
        let headers = self.signer.get_headers();

        let resp = self.http_client
            .post(&url)
            .headers(headers)
            .body(signed_body)
            .send()
            .await
            .map_err(|e| EngineError::ExchangeError(e.to_string()))?;

        // 4. Handle Response
        if resp.status().is_success() {
            let text = resp.text().await.map_err(|e| EngineError::ExchangeError(e.to_string()))?;
            Ok(text)
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| format!("Status: {}", status));
            Err(EngineError::ExchangeError(text))
        }
    }

    /// Fetch current position risk (positions).
    pub async fn get_position_risk(&self) -> Result<String, EngineError> {
        if !self.check_rate_limit() {
            return Err(EngineError::ExchangeError("Internal Rate Limit Exceeded".to_string()));
        }

        let timestamp = chrono::Utc::now().timestamp_millis();
        let query = format!("recvWindow=5000&timestamp={}", timestamp);
        let signature = self.signer.sign(&query);
        let signed_query = format!("{}&signature={}", query, signature);

        let url = format!("{}/fapi/v2/positionRisk?{}", self.base_url, signed_query);
        let headers = self.signer.get_headers();

        let resp = self.http_client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| EngineError::ExchangeError(e.to_string()))?;

        if resp.status().is_success() {
            let text = resp.text().await.map_err(|e| EngineError::ExchangeError(e.to_string()))?;
            Ok(text)
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| format!("Status: {}", status));
            Err(EngineError::ExchangeError(text))
        }
    }

    /// Cancel all open orders for a symbol.
    pub async fn cancel_all_orders(&self, symbol: &str) -> Result<String, EngineError> {
        // Note: We might want to bypass rate limit for emergency cancel, but for now we enforce it.
        // If we are spamming, we might get banned anyway.
        if !self.check_rate_limit() {
             return Err(EngineError::ExchangeError("Internal Rate Limit Exceeded".to_string()));
        }

        let timestamp = chrono::Utc::now().timestamp_millis();
        let query = format!("symbol={}&recvWindow=5000&timestamp={}", symbol.to_uppercase(), timestamp);
        let signature = self.signer.sign(&query);
        let signed_body = format!("{}&signature={}", query, signature);

        let url = format!("{}/fapi/v1/allOpenOrders", self.base_url);
        let headers = self.signer.get_headers();

        let resp = self.http_client
            .delete(&url)
            .headers(headers)
            .body(signed_body) // DELETE with body is supported by Binance
            .send()
            .await
            .map_err(|e| EngineError::ExchangeError(e.to_string()))?;

        if resp.status().is_success() {
            let text = resp.text().await.map_err(|e| EngineError::ExchangeError(e.to_string()))?;
            Ok(text)
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| format!("Status: {}", status));
            Err(EngineError::ExchangeError(text))
        }
    }

    pub fn sign_query(&self, query: &str) -> String {
        let (_signed_query, signature) = self.signer.sign_with_timestamp(query.to_string());
        format!("{}&signature={}", query, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::Side;

    #[tokio::test]
    async fn test_place_order_dry_run() {
        let client = ExecutionClient::new(
            "dummy_key".to_string(), 
            "dummy_secret".to_string(),
            "https://testnet.binancefuture.com".to_string()
        );

        let instr = TradeInstruction {
            symbol: "BTCUSDT".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            price: 50001.0,
            quantity: 0.01,
            timestamp: 123456789,
            dry_run: true,
        };

        let result = client.place_order(&instr).await;
        assert_eq!(result.unwrap(), "DRY_RUN_SUCCESS");
    }
    
    #[test]
    fn test_fmt_decimal() {
        assert_eq!(ExecutionClient::fmt_decimal(0.01000000), "0.01");
        assert_eq!(ExecutionClient::fmt_decimal(50000.00), "50000");
        assert_eq!(ExecutionClient::fmt_decimal(1.23456789), "1.23456789");
    }
}
