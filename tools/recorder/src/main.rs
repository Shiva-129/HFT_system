use anyhow::Context;
use std::path::Path;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path_str = "../../data/fixtures/raw_ticks.jsonl";
    let path = Path::new(path_str);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create fixtures directory")?;
    }

    let mut file = File::create(path)
        .await
        .context("Failed to create raw_ticks.jsonl")?;

    println!("Recording to {}...", path_str);

    let (raw_tx, mut raw_rx) = mpsc::channel::<String>(10_000);

    // Spawn writer task
    let _writer_handle = tokio::spawn(async move {
        let mut count = 0;
        while let Some(line) = raw_rx.recv().await {
            if let Err(e) = file.write_all(format!("{}\n", line).as_bytes()).await {
                eprintln!("Failed to write to file: {}", e);
                break;
            }
            count += 1;
            if count % 10 == 0 {
                println!("Recorded {} ticks", count);
            }
        }
        println!("Writer task finished. Total recorded: {}", count);
    });

    // Connect to Binance
    let _rx = feed_handler::connect("BTCUSDT", Some(raw_tx)).await?;

    println!("Connected to Binance. Recording for 60 seconds...");
    tokio::time::sleep(Duration::from_secs(60)).await;

    println!("Recording complete.");

    // Give writer task a moment to flush
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok(())
}
