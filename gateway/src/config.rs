use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub operator: Operator,
    #[serde(default)]
    pub gateway: Gateway,
    #[serde(default, rename = "service")]
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Operator {
    pub receive_address: String,
    pub usdc_mint: String,
    pub rpc_url: String,
    #[serde(default = "default_quote_ttl")]
    pub quote_ttl_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gateway {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            data_dir: default_data_dir(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub id: String,
    pub summary: String,
    /// Price in USDC (UI units). Converted to micro-USDC internally.
    pub price: f64,
    pub unit: String,
    pub adapter: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Service {
    /// USDC has 6 decimals; all on-chain comparison happens in raw micro-units.
    pub fn price_micros(&self) -> u64 {
        (self.price * 1_000_000.0).round() as u64
    }
}

impl Config {
    pub fn service(&self, id: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.enabled && s.id == id)
    }
}

pub fn load(path: &str) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config file {path}"))?;
    let cfg: Config = toml::from_str(&raw).context("invalid services.toml")?;
    if cfg.operator.receive_address == "YOUR_SOLANA_ADDRESS" {
        bail!("set operator.receive_address in {path} before starting");
    }
    if cfg.services.iter().filter(|s| s.enabled).count() == 0 {
        bail!("no enabled [[service]] blocks in {path}");
    }
    for s in cfg.services.iter().filter(|s| s.enabled) {
        if s.adapter != "command" {
            bail!("service {}: unknown adapter type {:?} (only \"command\" is supported)", s.id, s.adapter);
        }
    }
    Ok(cfg)
}

fn default_quote_ttl() -> u64 {
    300
}
fn default_bind() -> String {
    "127.0.0.1:4020".into()
}
fn default_data_dir() -> String {
    "data".into()
}
fn default_timeout() -> u64 {
    120
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_catalog() {
        let raw = include_str!("../../services.example.toml");
        let cfg: Config = toml::from_str(raw).expect("example toml must stay parseable");
        assert!(cfg.services.len() >= 2);
        let gpu = cfg.services.iter().find(|s| s.id == "gpu-inference").unwrap();
        assert_eq!(gpu.price_micros(), 250_000);
        let transcode = cfg.services.iter().find(|s| s.id == "transcode").unwrap();
        assert!(!transcode.enabled);
    }

    #[test]
    fn price_rounds_to_micros() {
        let s = Service {
            id: "x".into(),
            summary: String::new(),
            price: 0.1,
            unit: "per_request".into(),
            adapter: "command".into(),
            command: "true".into(),
            timeout_secs: 1,
            enabled: true,
        };
        assert_eq!(s.price_micros(), 100_000);
    }
}
