use common::{MarketEvent, Side};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

#[test]
fn test_end_to_end_pipeline() {
    // 1. Setup
    let shutdown = Arc::new(AtomicBool::new(false));

    // Create Queues
    let (mut market_prod, market_cons) = rtrb::RingBuffer::<MarketEvent>::new(32);
    let (trade_prod, mut trade_cons) = rtrb::RingBuffer::<common::TradeInstruction>::new(32);

    // 2. Spawn Strategy
    let s_flag = shutdown.clone();
    let strategy_handle = std::thread::spawn(move || {
        strategy::run(market_cons, trade_prod, s_flag);
    });

    // 3. Inject Events

    // Event A: Should NOT trigger (Price <= 50,000)
    let event_a = MarketEvent {
        symbol: "BTCUSDT".into(),
        price: 49_000.0,
        quantity: 1.0,
        exchange_timestamp: 1000,
        received_timestamp: common::now_nanos(),
    };
    market_prod.push(event_a).expect("Failed to push event A");

    // Event B: Should TRIGGER (Price > 50,000)
    let event_b = MarketEvent {
        symbol: "BTCUSDT".into(),
        price: 50_001.0,
        quantity: 1.0,
        exchange_timestamp: 2000,
        received_timestamp: common::now_nanos(),
    };
    market_prod.push(event_b).expect("Failed to push event B");

    // 4. Poll for Result (with timeout)
    let start = Instant::now();
    let result = loop {
        if let Ok(instr) = trade_cons.pop() {
            break Some(instr);
        }
        if start.elapsed() > Duration::from_millis(200) {
            break None;
        }
        std::hint::spin_loop();
    };

    // 5. Validate
    let instr = result.expect("Expected exactly one trade instruction");
    assert_eq!(instr.symbol, "BTCUSDT");
    assert_eq!(instr.side, Side::Buy);
    assert_eq!(instr.price, 50_001.0);
    assert_eq!(instr.quantity, 0.01);
    assert!(instr.dry_run);
    assert!(instr.timestamp > 0);

    // 6. Shutdown
    shutdown.store(true, Ordering::Relaxed);
    strategy_handle.join().expect("Strategy thread panicked");
}
