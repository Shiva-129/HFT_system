use anyhow::Context;
use feed_handler::parse_trade;
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use tokio_stream::StreamExt;

async fn generate_sample_data() -> anyhow::Result<()> {
    let path_str = "../../data/fixtures/raw_ticks.jsonl";
    let path = Path::new(path_str);

    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create fixtures directory")?;
    }

    let mut file = File::create(path)
        .await
        .context("Failed to create raw_ticks.jsonl")?;
    use tokio::io::AsyncWriteExt;

    for i in 0..100 {
        let price = 50000.0 + (i as f64 * 0.5);
        let timestamp = 1700000000000i64 + (i * 100);

        let record = json!({
            "e": "aggTrade",
            "s": "BTCUSDT",
            "p": format!("{:.1}", price),
            "q": "0.1",
            "T": timestamp
        });

        let line = format!("{}\n", record.to_string());
        file.write_all(line.as_bytes()).await?;
    }

    println!("Generated sample data at {}", path_str);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    generate_sample_data().await?;

    let path = "../../data/fixtures/raw_ticks.jsonl";
    let file = File::open(path)
        .await
        .context("Failed to open raw_ticks.jsonl")?;
    let reader = BufReader::new(file);
    let mut lines = LinesStream::new(reader.lines());

    let mut total_lines = 0;
    let mut success_count = 0;
    let mut error_count = 0;
    let start_time = Instant::now();

    println!("Starting replay...");

    while let Some(line) = lines.next().await {
        let line = line?;
        total_lines += 1;

        match parse_trade(&line) {
            Ok(event) => {
                success_count += 1;
                if success_count <= 5 {
                    println!("Parsed: {:?}", event);
                }
            }
            Err(e) => {
                error_count += 1;
                eprintln!("Error parsing line {}: {}", total_lines, e);
            }
        }
    }

    let duration = start_time.elapsed();

    println!("\n--- Replay Summary ---");
    println!("Total Processed: {}", total_lines);
    println!("Success: {}", success_count);
    println!("Errors: {}", error_count);
    println!("Duration: {:.2?}", duration);

    Ok(())
}
