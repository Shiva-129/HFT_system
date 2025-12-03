# HFT System 🚀

Build your high-frequency trading system in minutes. This Rust-based HFT engine harnesses real-time market data, advanced strategies, and lightning-fast execution to automate market analysis, risk management, and trade execution on Binance Futures.

## 🌟 Features

### Multi-Strategy Architecture
Leverages specialized trading strategies for different market conditions:
- **Ping Pong Strategy**: Simple alternating buy/sell for testing
- **Momentum Strategy**: Detects price velocity with fee-aware thresholds
- **Liquidation Strategy**: Hunts panic moves using volume-weighted cascade detection

### Real-Time Market Analysis
- WebSocket integration with Binance Futures for sub-millisecond latency
- Live price and volume tracking with 50-tick rolling windows
- Automatic position synchronization and balance updates

### Risk-First Approach
- Built-in risk engine with position limits and order size validation
- Transaction fee integration (maker/taker) in P&L calculations
- Auto-stop on max loss or target profit thresholds
- Real-time unrealized P&L tracking

### Professional Dashboard
- Real-time P&L visualization with Chart.js
- Interactive zoom/pan controls for historical analysis
- Live position, balance, and trade metrics
- System logs and trade timeline with grouped history
- Strategy switching and engine controls

### High-Performance Architecture
- Core affinity pinning for strategy and feed threads
- Lock-free ring buffers (RTRB) for event passing
- Async execution with Tokio runtime
- SQLite with WAL mode for trade persistence

## 📋 Requirements

- **Rust**: 1.70+ (2021 edition)
- **Binance Futures Account**: Testnet or mainnet API keys
- **Dependencies**: Listed in `Cargo.toml`
  - `tokio` - Async runtime
  - `axum` - Web server for dashboard
  - `sqlx` - Database operations
  - `serde` - Serialization
  - `tracing` - Structured logging
  - `chart.js` - Dashboard visualization

## 🚀 Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/Shiva-129/HFT_system.git
cd HFT_system

# Build the project
cargo build --release
```

### Configuration

1. Copy the example config:
```bash
cp config.example.toml config.toml
```

2. Edit `config.toml` with your settings:
```toml
[network]
name = "testnet"
rest_url = "https://testnet.binancefuture.com"
ws_url = "wss://stream.binancefuture.com/ws"

[trading]
api_key = "YOUR_API_KEY"
secret_key = "YOUR_SECRET_KEY"
enabled = true
dry_run = false

# Fee Configuration
fee_maker = 0.0002  # 0.02%
fee_taker = 0.0005  # 0.05%

# Strategy Parameters
strategy_window = 100       # Price history window (ticks)
strategy_threshold = 1.0    # Momentum threshold ($)
price_threshold = 10.0      # Liquidation price move ($)
volume_multiplier = 3.0     # Volume spike multiplier

[risk]
max_position = 1.0          # Maximum position size (BTC)
max_drawdown = 1000.0       # Maximum loss limit ($)
max_order_size = 0.1        # Maximum order size (BTC)
```

### Basic Usage

```bash
# Run the trading engine
cargo run -p trading_engine

# Access the dashboard
# Open browser to http://localhost:3000
```

## 🏗️ Architecture

### System Components

```
┌─────────────────────────────────────────────────────────────┐
│                      Trading Engine                          │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │  Feed Task   │───▶│   Strategy   │───▶│  Execution   │  │
│  │  (WebSocket) │    │   Thread     │    │    Task      │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│         │                    │                    │          │
│         │                    │                    │          │
│         ▼                    ▼                    ▼          │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Engine State (Shared)                    │  │
│  │  • Position  • P&L  • Balance  • Logs  • Metrics     │  │
│  └──────────────────────────────────────────────────────┘  │
│                              │                               │
│                              ▼                               │
│                    ┌──────────────────┐                     │
│                    │   Web Server     │                     │
│                    │   (Dashboard)    │                     │
│                    └──────────────────┘                     │
└─────────────────────────────────────────────────────────────┘
```

### Strategy Flow

1. **Feed Task**: Receives market events via WebSocket
2. **Strategy Thread**: Processes events and generates trade signals
3. **Risk Engine**: Validates signals against risk parameters
4. **Execution Task**: Places orders on Binance
5. **State Update**: Updates P&L, position, and balance
6. **Dashboard**: Displays real-time metrics via SSE

## 📊 Strategy Details

### Liquidation Strategy (Recommended)

Detects panic moves (liquidation cascades) using price velocity + volume spikes:

**Entry Conditions:**
- Price velocity > `price_threshold` (e.g., $10 move over 50 ticks)
- Volume burst > `volume_multiplier` × average (e.g., 3x normal volume)

**Exit Conditions:**
- Volume normalizes (recent average ≤ rolling average)

**Example Configuration:**
```toml
price_threshold = 10.0      # Minimum $10 price move
volume_multiplier = 3.0     # 3x volume spike required
```

### Momentum Strategy

Detects sustained price trends with fee-aware thresholds:

**Entry Conditions:**
- Price velocity > `threshold + fees` (upward trend)
- Price velocity < `-threshold - fees` (downward trend)

**Exit Conditions:**
- Velocity reverses (crosses zero)

**Example Configuration:**
```toml
strategy_window = 100       # Look back 100 ticks
strategy_threshold = 1.0    # Base threshold $1
```

## 🔧 Advanced Configuration

### Database

The system uses SQLite with WAL mode for trade persistence:

```rust
// Automatic schema creation
CREATE TABLE IF NOT EXISTS trades (
    id INTEGER PRIMARY KEY,
    exchange_ts_ms INTEGER,
    symbol TEXT,
    side TEXT,
    price REAL,
    quantity REAL,
    pnl REAL,
    fee REAL,
    strategy TEXT
)
```

### Logging

Structured logging with `tracing`:

```bash
# Set log level
RUST_LOG=info cargo run -p trading_engine

# Available levels: trace, debug, info, warn, error
```

### Performance Tuning

**Core Affinity:**
- Strategy thread pinned to last CPU core
- Feed task runs on async runtime
- Execution task runs on async runtime

**Ring Buffer Sizes:**
```rust
// Adjust in main.rs
let (producer, consumer) = rtrb::RingBuffer::new(4096);  // Market events
let (signal_producer, signal_consumer) = rtrb::RingBuffer::new(4096);  // Trade signals
```

## 📈 Dashboard Features

### Real-Time Metrics
- **Total P&L**: Realized profits/losses
- **Unrealized P&L**: Current position value
- **Position**: Current BTC position
- **Available Balance**: USDT available for trading

### P&L Chart
- Interactive zoom and pan
- Time range filters (Session, Today, Month, Year)
- Historical data from database

### Controls
- **Start/Stop Engine**: Enable/disable trading
- **Strategy Selector**: Switch between strategies
- **Flatten All**: Emergency position close
- **Clear History**: Reset session data

### System Logs
- Real-time trade execution logs
- Strategy signals and exits
- Risk rejections and errors

## 🔍 Monitoring & Debugging

### Debug Logging

Enable debug logs in strategies:

```rust
// Liquidation Strategy
LIQUIDATION Debug: Velocity=20.00, PriceThreshold=10.00, 
  RecentVolAvg=0.0562, RollingVolAvg=0.0406, 
  VolMultiplier=3.0x, Position=0

// Momentum Strategy
Momentum Debug: Velocity=9.00, Threshold=94.91 
  (Base=2.00 + Fee=92.91)
```

### Trade Analysis

Query historical trades:

```bash
# Access SQLite database
sqlite3 trades.db

# View recent trades
SELECT * FROM trades ORDER BY exchange_ts_ms DESC LIMIT 10;

# Calculate total P&L
SELECT SUM(pnl) as total_pnl FROM trades;

# Analyze by strategy
SELECT strategy, COUNT(*), SUM(pnl) 
FROM trades 
GROUP BY strategy;
```

## ⚠️ Risk Management

### Position Limits
```toml
max_position = 1.0      # Maximum 1 BTC position
max_order_size = 0.1    # Maximum 0.1 BTC per order
```

### Auto-Stop
```toml
max_drawdown = 1000.0   # Stop if loss exceeds $1000
```

The engine automatically stops trading when:
- Loss exceeds `max_drawdown`
- Profit exceeds `target_profit` (if set)

### Fee Awareness

All strategies account for transaction fees:
- Momentum: Adds fee cost to threshold
- Liquidation: Requires larger moves to cover fees

## 🐛 Troubleshooting

### Common Issues

**"Failed to parse config.toml"**
- Ensure all required fields are present
- Check TOML syntax (no trailing commas)

**"AUTH_ERROR"**
- Verify API keys are correct
- Check API key permissions (Futures trading enabled)

**"Strategy not trading"**
- Check debug logs for velocity/volume values
- Ensure thresholds are appropriate for market conditions
- Verify fees aren't making threshold too high

**"P&L always dropping"**
- Check fee configuration (`fee_taker`)
- Increase `price_threshold` or `strategy_threshold`
- Reduce `volume_multiplier` for more frequent exits

## 📝 Development

### Project Structure

```
hft_system/
├── apps/
│   └── trading_engine/     # Main trading engine
│       ├── src/
│       │   ├── main.rs     # Entry point
│       │   ├── config.rs   # Configuration
│       │   ├── state.rs    # Shared state
│       │   ├── server.rs   # Web server
│       │   └── db.rs       # Database
│       └── Cargo.toml
├── crates/
│   ├── common/             # Shared types
│   ├── execution/          # Order execution
│   ├── feed/               # Market data
│   ├── risk/               # Risk management
│   └── strategy/           # Trading strategies
│       ├── src/
│       │   ├── lib.rs      # Strategy runner
│       │   ├── ping_pong.rs
│       │   ├── momentum.rs
│       │   └── liquidation.rs
│       └── Cargo.toml
├── dashboard/
│   └── index.html          # Web dashboard
├── config.toml             # Configuration (gitignored)
├── config.example.toml     # Example config
└── README.md
```

### Adding a New Strategy

1. Create strategy file in `crates/strategy/src/`:
```rust
use common::{MarketEvent, TradeInstruction};
use crate::Strategy;

pub struct MyStrategy {
    // Strategy state
}

impl Strategy for MyStrategy {
    fn process_event(&mut self, event: &MarketEvent) -> Option<TradeInstruction> {
        // Strategy logic
        None
    }
}
```

2. Register in `lib.rs`:
```rust
mod my_strategy;
use my_strategy::MyStrategy;

fn create_strategy(name: &str, ...) -> Box<dyn Strategy> {
    match name {
        "MY_STRATEGY" => Box::new(MyStrategy::new()),
        // ...
    }
}
```

3. Add to dashboard strategy selector in `dashboard/index.html`

## 🤝 Contributing

Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run `cargo fmt` and `cargo clippy`
5. Submit a pull request

## 📄 License

This project is for educational purposes. Use at your own risk.

## ⚡ Performance Metrics

- **Latency**: Sub-millisecond event processing
- **Throughput**: 10,000+ ticks/second
- **Memory**: ~50MB typical usage
- **CPU**: Single-threaded strategy, async I/O

## 🔗 Resources

- [Binance Futures API Docs](https://binance-docs.github.io/apidocs/futures/en/)
- [Rust Async Book](https://rust-lang.github.io/async-book/)
- [Chart.js Documentation](https://www.chartjs.org/docs/)

## 📧 Support

For issues and questions:
- GitHub Issues: [HFT_system/issues](https://github.com/Shiva-129/HFT_system/issues)
- Documentation: See this README

---

**⚠️ Disclaimer**: This software is for educational purposes only. Trading cryptocurrencies carries significant risk. Always test on testnet before using real funds. The authors are not responsible for any financial losses.
