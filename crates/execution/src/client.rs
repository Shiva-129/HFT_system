use crate::signer::BinanceSigner;
use common::{EngineError, OrderType, TradeInstruction};
use governor::{DefaultDirectRateLimiter, Quota};
use nonzero_ext::nonzero;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

pub struct ExecutionClient {
    http_client: Client,
    signer: BinanceSigner,
    base_url: String,
    // Rate Limiting: 10 requests per second, burst 10
    rate_limiter: DefaultDirectRateLimiter,
}

#[derive(Debug, Deserialize)]
pub struct PositionRisk {
    pub symbol: String,
    #[serde(rename = "positionAmt")]
    pub position_amt: String,
    #[serde(rename = "entryPrice")]
    pub entry_price: String,
    #[serde(rename = "markPrice")]
    pub mark_price: String,
}

impl ExecutionClient {
    pub fn new(api_key: String, secret_key: String, base_url: String) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        // Rate limit: 10 req/s, burst 10
        let rate_limiter = DefaultDirectRateLimiter::direct(
            Quota::per_second(nonzero!(10u32)).allow_burst(nonzero!(10u32)),
        );

        Self {
            http_client,
            signer: BinanceSigner::new(api_key, secret_key),
            base_url,
            rate_limiter,
        }
    }

    /// Helper to format decimals: 8 decimal places, trim trailing zeros and dot.
    fn fmt_decimal(v: f64) -> String {
        let s = format!("{:.8}", v);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }

    /// Helper to check rate limit asynchronously.
    async fn await_rate_limit(&self) {
        self.rate_limiter.until_ready().await;
    }

    /// Place an order. If instruction.dry_run == true, return Ok("DRY_RUN_SUCCESS").
    pub async fn place_order(&self, instruction: &TradeInstruction) -> Result<String, EngineError> {
        if instruction.dry_run {
            return Ok("DRY_RUN_SUCCESS".to_string());
        }

        self.await_rate_limit().await;

        // 1. Build Canonical Query String
        // Order: symbol, side, type, quantity, timeInForce (if Limit), price (if Limit), recvWindow, timestamp
        let mut query = format!(
            "symbol={}&side={}&type={}&quantity={}",
            instruction.symbol.to_uppercase(),
            format!("{:?}", instruction.side).to_uppercase(),
            format!("{:?}", instruction.order_type).to_uppercase(),
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

        let resp = self
            .http_client
            .post(&url)
            .headers(headers)
            .body(signed_body)
            .send()
            .await
            .map_err(|e| EngineError::ExchangeError(e.to_string()))?;

        // 4. Handle Response
        if resp.status().is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| EngineError::ExchangeError(e.to_string()))?;
            Ok(text)
        } else {
            let status = resp.status();
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| format!("Status: {}", status));

            // Check for Auth errors (-2014, -2015, etc.)
            if text.contains("-2014")
                || text.contains("-2015")
                || text.contains("API-key format invalid")
            {
                return Err(EngineError::ExchangeError(format!("AUTH_ERROR: {}", text)));
            }

            Err(EngineError::ExchangeError(text))
        }
    }

    /// Fetch current position risk (positions).
    pub async fn sync_positions(&self) -> Result<Vec<PositionRisk>, EngineError> {
        self.await_rate_limit().await;

        let timestamp = chrono::Utc::now().timestamp_millis();
        let query = format!("recvWindow=5000&timestamp={}", timestamp);
        let signature = self.signer.sign(&query);
        let signed_query = format!("{}&signature={}", query, signature);

        let url = format!("{}/fapi/v2/positionRisk?{}", self.base_url, signed_query);
        let headers = self.signer.get_headers();

        let resp = self
            .http_client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| EngineError::ExchangeError(e.to_string()))?;

        if resp.status().is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| EngineError::ExchangeError(e.to_string()))?;
            let positions: Vec<PositionRisk> = serde_json::from_str(&text).map_err(|e| {
                EngineError::ExchangeError(format!("Failed to parse positions: {}", e))
            })?;
            Ok(positions)
        } else {
            let status = resp.status();
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| format!("Status: {}", status));

            // Check for Auth errors
            if text.contains("-2014")
                || text.contains("-2015")
                || text.contains("API-key format invalid")
            {
                return Err(EngineError::ExchangeError(format!("AUTH_ERROR: {}", text)));
            }

            Err(EngineError::ExchangeError(text))
        }
    }

    /// Cancel all open orders for a symbol.
    /// Retries up to 3 times on network failure.
    pub async fn cancel_all_orders(&self, symbol: &str) -> Result<(), EngineError> {
        let max_retries = 3;
        let mut last_error = String::new();

        for attempt in 1..=max_retries {
            self.await_rate_limit().await;

            let timestamp = chrono::Utc::now().timestamp_millis();
            let query = format!(
                "symbol={}&recvWindow=5000&timestamp={}",
                symbol.to_uppercase(),
                timestamp
            );
            let signature = self.signer.sign(&query);
            let signed_body = format!("{}&signature={}", query, signature);

            let url = format!("{}/fapi/v1/allOpenOrders", self.base_url);
            let headers = self.signer.get_headers();

            // Use a shorter timeout for cancel requests to avoid hanging shutdown
            let client_with_timeout =
                match Client::builder().timeout(Duration::from_secs(5)).build() {
                    Ok(c) => c,
                    Err(_) => self.http_client.clone(), // Fallback
                };

            let result = client_with_timeout
                .delete(&url)
                .headers(headers.clone())
                .body(signed_body)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(());
                    } else if resp.status().as_u16() == 400 {
                        let text = resp.text().await.unwrap_or_default();
                        if text.contains("No open order") || text.contains("-2011") {
                            // "Unknown order sent" or similar often means no orders to cancel
                            return Ok(());
                        }
                        last_error = text;
                    } else {
                        let status = resp.status();
                        let text = resp
                            .text()
                            .await
                            .unwrap_or_else(|_| format!("Status: {}", status));
                        last_error = text;
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                }
            }

            if attempt < max_retries {
                // Exponential backoff: 100ms, 200ms, 400ms
                tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(attempt as u32 - 1))).await;
            }
        }

        Err(EngineError::ExchangeError(format!(
            "Failed to cancel orders after {} attempts: {}",
            max_retries, last_error
        )))
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
            "https://testnet.binancefuture.com".to_string(),
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
