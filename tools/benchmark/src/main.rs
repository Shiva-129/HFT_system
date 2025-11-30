use anyhow::Context;
use common::{MarketEvent, TradeInstruction};
use feed_handler::parse_trade;
use hdrhistogram::Histogram;
use std::fs;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;

fn load_ticks() -> anyhow::Result<Vec<String>> {
    let path = "../../data/fixtures/raw_ticks.jsonl";
    let content = fs::read_to_string(path).context("Failed to read raw_ticks.jsonl")?;
    Ok(content.lines().map(|s| s.to_string()).collect())
}

fn bench_parsing(ticks: &[String]) -> Histogram<u64> {
    let mut hist = Histogram::<u64>::new_with_bounds(1, 100_000_000, 3).unwrap();
    let iterations = 1_000_000;
    let tick_count = ticks.len();

    println!("Running Parse Benchmark ({} iterations)...", iterations);

    for i in 0..iterations {
        let line = &ticks[i % tick_count];
        let start = Instant::now();
        let _ = parse_trade(line).unwrap();
        let elapsed = start.elapsed().as_nanos() as u64;
        hist.record(elapsed).unwrap();
    }
    hist
}

fn bench_e2e(ticks: &[String]) -> Histogram<u64> {
    let mut hist = Histogram::<u64>::new_with_bounds(1, 100_000_000, 3).unwrap(); // Up to 100ms
    let iterations = 100_000;
    let tick_count = ticks.len();

    println!(
        "Running End-to-End Benchmark ({} iterations)...",
        iterations
    );

    // Setup Pipeline
    let (mut market_prod, market_cons) = rtrb::RingBuffer::<MarketEvent>::new(4096);
    let (trade_prod, mut trade_cons) = rtrb::RingBuffer::<TradeInstruction>::new(4096);
    let shutdown = Arc::new(AtomicBool::new(false));
    let is_running = Arc::new(AtomicBool::new(true));

    // Spawn Strategy (No Throttle)
    let s_shutdown = shutdown.clone();
    let s_running = is_running.clone();
    std::thread::spawn(move || {
        // Pin to the last available core
        if let Some(core_ids) = core_affinity::get_core_ids() {
            if let Some(core_id) = core_ids.last() {
                core_affinity::set_for_current(*core_id);
            }
        }
        strategy::run(market_cons, trade_prod, s_shutdown, s_running, true, true);
        // dry_run=true, disable_throttle=true
    });

    for i in 0..iterations {
        let line = &ticks[i % tick_count];

        // 1. Parse & Timestamp
        let mut event = parse_trade(line).unwrap();
        event.received_timestamp = common::now_nanos(); // Start Clock
        let start_ts = event.received_timestamp;

        // 2. Push to Strategy
        // If buffer full, spin
        while market_prod.is_full() {
            std::hint::spin_loop();
        }
        market_prod.push(event).unwrap();

        // 3. Wait for Output
        loop {
            if let Ok(_instr) = trade_cons.pop() {
                // 4. Measure Latency
                let end_ts = common::now_nanos();
                // Use instruction timestamp if we want "Strategy Decision Time",
                // but for "System Latency" we want "Time until Execution receives it".
                // Let's measure "Tick-to-Order" (End-to-End).
                let latency = end_ts.saturating_sub(start_ts);
                hist.record(latency).unwrap();
                break;
            }
            std::hint::spin_loop();
        }
    }

    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
    hist
}

fn print_stats(name: &str, hist: &Histogram<u64>) {
    println!("\n--- {} Results (ns) ---", name);
    println!("Min:    {}", hist.min());
    println!("p50:    {}", hist.value_at_quantile(0.50));
    println!("p95:    {}", hist.value_at_quantile(0.95));
    println!("p99:    {}", hist.value_at_quantile(0.99));
    println!("Max:    {}", hist.max());
    println!("Mean:   {:.2}", hist.mean());
}

fn main() -> anyhow::Result<()> {
    let ticks = load_ticks()?;
    println!("Loaded {} ticks for benchmarking.", ticks.len());

    let parse_hist = bench_parsing(&ticks);
    print_stats("Tick-to-Parse", &parse_hist);

    let e2e_hist = bench_e2e(&ticks);
    print_stats("End-to-End (Tick-to-Order)", &e2e_hist);

    Ok(())
}
