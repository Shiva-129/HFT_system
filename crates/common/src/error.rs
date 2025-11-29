use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Parse Error: {0}")]
    ParseError(String),
    #[error("Risk Violation: {0}")]
    RiskViolation(String),
    #[error("Queue Full")]
    QueueFull,
    #[error("Exchange Error: {0}")]
    ExchangeError(String),
}
