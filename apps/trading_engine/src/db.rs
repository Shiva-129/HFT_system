use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub exchange_ts_ms: i64,
    pub monotonic_ns: u64,
    pub symbol: String,
    pub side: String,
    pub price: f64,
    pub quantity: f64,
    pub pnl: f64,
    pub strategy: String,
    pub order_id: Option<String>,
    pub exec_id: Option<String>,
    pub fee: Option<f64>,
    pub fee_currency: Option<String>,
    pub raw: Option<String>,
}

#[derive(Clone)]
pub struct TradeStorage {
    pool: Pool<Sqlite>,
    tx: mpsc::Sender<TradeRecord>,
}

impl TradeStorage {
    pub async fn new(path: &str) -> anyhow::Result<Self> {
        let db_url = format!("sqlite:{}", path);

        // 1. Configure Options
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(&db_url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        // 2. Connect
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;

        // 3. Create Table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trades (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                exchange_ts_ms INTEGER,
                monotonic_ns INTEGER,
                symbol TEXT,
                side TEXT,
                price REAL,
                quantity REAL,
                pnl REAL,
                strategy TEXT,
                order_id TEXT,
                exec_id TEXT,
                fee REAL,
                fee_currency TEXT,
                raw TEXT
            );
            "#,
        )
        .execute(&pool)
        .await?;

        // 4. Spawn Writer Task
        let (tx, mut rx) = mpsc::channel::<TradeRecord>(10_000);
        let pool_clone = pool.clone();

        tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(100);
            let mut last_flush = Instant::now();
            let flush_interval = Duration::from_millis(100);

            loop {
                match rx.recv().await {
                    Some(record) => {
                        buffer.push(record);

                        let should_flush =
                            buffer.len() >= 100 || last_flush.elapsed() >= flush_interval;

                        if should_flush {
                            if let Err(e) = Self::flush_buffer(&pool_clone, &buffer).await {
                                tracing::error!("Failed to flush trades to DB: {}", e);
                            }
                            buffer.clear();
                            last_flush = Instant::now();
                        }
                    }
                    None => {
                        // Channel closed, flush remaining
                        if !buffer.is_empty() {
                            if let Err(e) = Self::flush_buffer(&pool_clone, &buffer).await {
                                tracing::error!("Failed to flush remaining trades: {}", e);
                            }
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self { pool, tx })
    }

    async fn flush_buffer(pool: &Pool<Sqlite>, buffer: &[TradeRecord]) -> anyhow::Result<()> {
        let mut tx = pool.begin().await?;

        for trade in buffer {
            sqlx::query(
                r#"
                INSERT INTO trades (
                    exchange_ts_ms, monotonic_ns, symbol, side, price, quantity, pnl, strategy,
                    order_id, exec_id, fee, fee_currency, raw
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(trade.exchange_ts_ms)
            .bind(trade.monotonic_ns as i64) // SQLite doesn't have u64
            .bind(&trade.symbol)
            .bind(&trade.side)
            .bind(trade.price)
            .bind(trade.quantity)
            .bind(trade.pnl)
            .bind(&trade.strategy)
            .bind(&trade.order_id)
            .bind(&trade.exec_id)
            .bind(trade.fee)
            .bind(&trade.fee_currency)
            .bind(&trade.raw)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn insert_trade(&self, trade: TradeRecord) {
        // Non-blocking send. If full, drop and log.
        match self.tx.try_send(trade) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("TradeStorage channel full! Dropping trade record.");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("TradeStorage channel closed! Cannot save trade.");
            }
        }
    }

    pub async fn get_recent_trades(&self, limit: i64) -> anyhow::Result<Vec<TradeRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                exchange_ts_ms, monotonic_ns, symbol, side, price, quantity, pnl, strategy,
                order_id, exec_id, fee, fee_currency, raw
            FROM trades 
            ORDER BY id DESC 
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            use sqlx::Row;
            trades.push(TradeRecord {
                exchange_ts_ms: row.try_get("exchange_ts_ms")?,
                monotonic_ns: row.try_get::<i64, _>("monotonic_ns")? as u64,
                symbol: row.try_get("symbol")?,
                side: row.try_get("side")?,
                price: row.try_get("price")?,
                quantity: row.try_get("quantity")?,
                pnl: row.try_get("pnl")?,
                strategy: row.try_get("strategy")?,
                order_id: row.try_get("order_id")?,
                exec_id: row.try_get("exec_id")?,
                fee: row.try_get("fee")?,
                fee_currency: row.try_get("fee_currency")?,
                raw: row.try_get("raw")?,
            });
        }
        Ok(trades)
    }

    pub async fn flush(&self) {
        // In a real implementation, we might send a special flush signal or wait for empty.
        // For now, we rely on the channel drop behavior in main to finish writing.
        // But to be safe, we can sleep briefly or implement a proper flush command.
        // Since main awaits handles, dropping the sender in main will cause the loop to exit
        // and flush remaining buffer.
    }
}
