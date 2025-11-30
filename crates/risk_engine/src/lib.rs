use common::{EngineError, TradeInstruction};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global Kill Switch
/// Default: false (Disarmed/Safe) - System will reject orders until explicitly armed.
pub static TRADING_ENABLED: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

/// Arm the system (Enable Trading)
pub fn arm() {
    tracing::warn!("SYSTEM ARMED - TRADING ENABLED");
    TRADING_ENABLED.store(true, Ordering::SeqCst);
}

/// Disarm the system (Disable Trading)
pub fn disarm() {
    tracing::warn!("SYSTEM DISARMED - TRADING DISABLED");
    TRADING_ENABLED.store(false, Ordering::SeqCst);
}

/// Check if system is armed
pub fn is_armed() -> bool {
    TRADING_ENABLED.load(Ordering::SeqCst)
}

pub struct RiskEngine;

impl RiskEngine {
    pub fn new() -> Self {
        RiskEngine
    }

    pub fn check(&mut self, instruction: &TradeInstruction) -> Result<(), EngineError> {
        // 1. Kill Switch
        if !is_armed() {
            return Err(EngineError::RiskViolation("System Disarmed".to_string()));
        }

        // 2. Dry-run bypass
        if instruction.dry_run {
            return Ok(());
        }

        // 3. Quantity
        if instruction.quantity <= 0.0 {
            return Err(EngineError::RiskViolation("Zero/Negative Quantity".to_string()));
        }

        // 4. Price
        if instruction.price <= 0.0 {
            return Err(EngineError::RiskViolation("Invalid Price".to_string()));
        }

        Ok(())
    }
}
