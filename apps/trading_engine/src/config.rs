use anyhow::Context;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct AppConfig {
    pub network: NetworkConfig,
    pub trading: TradingConfig,
    pub risk: RiskConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct NetworkConfig {
    pub name: String,
    pub rest_url: String,
    pub ws_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TradingConfig {
    pub api_key: Option<String>,
    pub secret_key: Option<String>,
    pub enabled: bool,
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RiskConfig {
    pub max_position: f64,
    pub max_drawdown: f64,
}

pub fn load(path: &str) -> Result<AppConfig, anyhow::Error> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path))?;

    let config: AppConfig =
        toml::from_str(&content).with_context(|| "Failed to parse config.toml")?;

    Ok(config)
}
