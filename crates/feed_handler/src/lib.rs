pub mod binance;
pub use binance::*;

use common::{MarketEvent, EngineError};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::StreamExt;
use url::Url;
use std::time::Duration;

pub async fn connect(symbol: &str) -> Result<mpsc::Receiver<MarketEvent>, EngineError> {
    let (tx, rx) = mpsc::channel::<MarketEvent>(10_000);
    let symbol_lower = symbol.to_lowercase();
    let url_str = format!("wss://fstream.binance.com/ws/{}@aggTrade", symbol_lower);
    
    // Validate URL upfront
    if Url::parse(&url_str).is_err() {
        return Err(EngineError::ParseError(format!("Invalid URL: {}", url_str)));
    }

    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(100);
        let max_backoff = Duration::from_secs(5);

        loop {
            let url = Url::parse(&url_str).expect("URL already validated");

            match connect_async(url).await {
                Ok((ws_stream, _)) => {
                    tracing::info!("Connected to Binance for {}", symbol_lower);
                    backoff = Duration::from_millis(100); // Reset backoff
                    
                    let (_, mut read) = ws_stream.split();

                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                match parse_trade(text.as_str()) {
                                    Ok(event) => {
                                        if let Err(_) = tx.try_send(event) {
                                            tracing::warn!("dropping tick due to backpressure");
                                            continue;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Parse error: {}", e);
                                    }
                                }
                            }
                            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                            Ok(Message::Close(_)) => {
                                tracing::warn!("WebSocket closed by server");
                                break;
                            }
                            Err(e) => {
                                tracing::error!("WebSocket error: {}", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Connection failed: {}. Retrying in {:?}", e, backoff);
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(backoff * 2, max_backoff);
        }
    });

    Ok(rx)
}
