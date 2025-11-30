use common::MarketEvent;
use rtrb::{PushError, PopError};
use std::thread;
use std::time::Instant;
use std::hint;

const TOTAL_MESSAGES: usize = 10_000_000;
const QUEUE_CAPACITY: usize = 4096;

fn main() {
    println!("Starting SPSC Ring Buffer Stress Test");
    println!("Messages: {}", TOTAL_MESSAGES);
    println!("Capacity: {}", QUEUE_CAPACITY);

    // Build ring buffer
    let (mut producer, mut consumer) = rtrb::RingBuffer::<MarketEvent>::new(QUEUE_CAPACITY);

    // Create dummy event
    let event = MarketEvent {
        symbol: "BTCUSDT".into(),
        price: 50000.0,
        quantity: 0.1,
        exchange_timestamp: 0,
        received_timestamp: 0,
    };

    let start = Instant::now();

    // Thread A — PRODUCER
    let producer_handle = thread::Builder::new()
        .name("producer".into())
        .spawn(move || {
            for _ in 0..TOTAL_MESSAGES {
                loop {
                    match producer.push(event.clone()) {
                        Ok(_) => break,
                        Err(PushError::Full(_)) => hint::spin_loop(),
                    }
                }
            }
        })
        .expect("Failed to spawn producer thread");

    // Thread B — CONSUMER
    let consumer_handle = thread::Builder::new()
        .name("consumer".into())
        .spawn(move || {
            let mut received = 0;
            while received < TOTAL_MESSAGES {
                match consumer.pop() {
                    Ok(_) => received += 1,
                    Err(PopError::Empty) => hint::spin_loop(),
                }
            }
            received
        })
        .expect("Failed to spawn consumer thread");

    producer_handle.join().expect("Producer thread panicked");
    let received_count = consumer_handle.join().expect("Consumer thread panicked");

    let duration = start.elapsed();
    let duration_secs = duration.as_secs_f64();
    let duration_ns = duration.as_nanos() as f64;
    
    let throughput = TOTAL_MESSAGES as f64 / duration_secs;
    let avg_ns = duration_ns / TOTAL_MESSAGES as f64;

    println!("\n--- Stress Test Results ---");
    println!("Total Processed: {}", received_count);
    println!("Duration: {:.2?}", duration);
    println!("Throughput: {:.2} msgs/sec", throughput);
    println!("Avg Handoff: {:.2} ns/msg", avg_ns);

    assert_eq!(received_count, TOTAL_MESSAGES, "Message count mismatch!");
    println!("\nSUCCESS: Zero drops confirmed.");
}
