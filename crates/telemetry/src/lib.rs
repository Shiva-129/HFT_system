use std::fs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize telemetry (logging).
///
/// - Creates `log_dir` if it doesn't exist.
/// - Sets up a daily rolling file appender (non-blocking).
/// - Configures a console layer (Compact, INFO+).
/// - Configures a file layer (JSON, DEBUG+).
/// - Returns a `WorkerGuard` that must be kept alive to ensure logs are flushed on exit.
pub fn init(log_dir: &str) -> WorkerGuard {
    // 1. Create log directory
    fs::create_dir_all(log_dir).expect("Failed to create log directory");

    // 2. File Appender (Daily Rolling)
    let file_appender = tracing_appender::rolling::daily(log_dir, "hft.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // 3. Console Layer (Compact, Human-readable)
    // Uses RUST_LOG env var if set, otherwise defaults to INFO.
    let console_layer = fmt::layer()
        .with_target(false)
        .with_level(true)
        .compact()
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")));

    // 4. File Layer (Structured JSON)
    // Captures more detail (DEBUG level by default).
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_filter(EnvFilter::new("debug"));

    // 5. Register Layers
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();

    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_telemetry_init() {
        let log_dir = "test_logs";
        let _guard = init(log_dir);

        tracing::info!("Test log message");

        // Give it a moment to flush (async writer)
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify file creation (simple check)
        let paths = fs::read_dir(log_dir).unwrap();
        let mut found = false;
        for path in paths {
            let p = path.unwrap().path();
            if p.extension().unwrap_or_default() == "log" || p.to_string_lossy().contains("hft.log")
            {
                found = true;
                break;
            }
        }
        // Cleanup
        let _ = fs::remove_dir_all(log_dir);

        assert!(found, "Log file should have been created");
    }
}
