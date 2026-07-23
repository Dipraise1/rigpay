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
    /// Plain public address where USDC lands. rigpay never sees its key.
    pub receive_address: String,
    pub usdc_mint: String,
    pub rpc_url: String,
    /// A quote's payment window. After this the reference key is dead and the
    /// client must re-quote — bounds how long we track any single quote.
    #[serde(default = "default_quote_ttl")]
    pub quote_ttl_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gateway {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// Hard cap on request payloads. Protects the adapter hosts, not just RAM.
    #[serde(default = "default_max_body_mb")]
    pub max_body_mb: usize,
    /// Cap on simultaneously outstanding quotes. Quote issuance costs an
    /// attacker nothing, so the map it lives in must have a ceiling.
    #[serde(default = "default_max_quotes")]
    pub max_outstanding_quotes: usize,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            data_dir: default_data_dir(),
            max_body_mb: default_max_body_mb(),
            max_outstanding_quotes: default_max_quotes(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub id: String,
    pub summary: String,
    /// Price in USDC (UI units). All comparison happens in raw micro-units —
    /// see [`Service::price_micros`].
    pub price: f64,
    pub unit: String,
    pub adapter: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Per-service concurrency budget. A saturated service answers 429 —
    /// there is deliberately no internal queue to grow unbounded.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Service {
    /// USDC has 6 decimals; on-chain amounts are raw micro-units.
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
    validate(&cfg)?;
    Ok(cfg)
}

/// Startup is the one place allowed to fail loudly: a config that would
/// misbehave under load is rejected before we ever bind a socket.
pub fn validate(cfg: &Config) -> Result<()> {
    if cfg.operator.receive_address == "YOUR_SOLANA_ADDRESS" {
        bail!("set operator.receive_address before starting");
    }
    if bs58::decode(&cfg.operator.receive_address)
        .into_vec()
        .map(|b| b.len())
        != Ok(32)
    {
        bail!("operator.receive_address is not a valid base58 Solana address");
    }
    if bs58::decode(&cfg.operator.usdc_mint).into_vec().map(|b| b.len()) != Ok(32) {
        bail!("operator.usdc_mint is not a valid base58 mint address");
    }
    if cfg.services.iter().filter(|s| s.enabled).count() == 0 {
        bail!("no enabled [[service]] blocks");
    }
    for s in cfg.services.iter().filter(|s| s.enabled) {
        if s.adapter != "command" {
            bail!(
                "service {}: unknown adapter type {:?} (only \"command\" is supported)",
                s.id,
                s.adapter
            );
        }
        if s.price < 0.0 || !s.price.is_finite() {
            bail!("service {}: price must be a finite non-negative number", s.id);
        }
        if s.max_concurrent == 0 {
            bail!("service {}: max_concurrent must be at least 1", s.id);
        }
        if s.timeout_secs == 0 {
            bail!("service {}: timeout_secs must be at least 1", s.id);
        }
    }
    Ok(())
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
fn default_max_body_mb() -> usize {
    16
}
fn default_max_quotes() -> usize {
    10_000
}
fn default_timeout() -> u64 {
    120
}
fn default_max_concurrent() -> usize {
    4
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Config {
        toml::from_str(include_str!("../../services.example.toml")).unwrap()
    }

    #[test]
    fn parses_example_catalog() {
        let cfg = base();
        assert!(cfg.services.len() >= 2);
        let gpu = cfg.services.iter().find(|s| s.id == "gpu-inference").unwrap();
        assert_eq!(gpu.price_micros(), 250_000);
        assert_eq!(gpu.max_concurrent, 4);
        assert!(!cfg.services.iter().find(|s| s.id == "transcode").unwrap().enabled);
    }

    #[test]
    fn placeholder_address_rejected() {
        let cfg = base();
        assert!(validate(&cfg).unwrap_err().to_string().contains("receive_address"));
    }

    #[test]
    fn invalid_base58_rejected() {
        let mut cfg = base();
        cfg.operator.receive_address = "not-base58-0OIl".into();
        assert!(validate(&cfg).is_err());
    }

    #[test]
    fn valid_config_passes() {
        let mut cfg = base();
        cfg.operator.receive_address = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into();
        validate(&cfg).unwrap();
    }

    #[test]
    fn zero_concurrency_rejected() {
        let mut cfg = base();
        cfg.operator.receive_address = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into();
        cfg.services[0].max_concurrent = 0;
        assert!(validate(&cfg).unwrap_err().to_string().contains("max_concurrent"));
    }

    #[test]
    fn price_rounds_to_micros() {
        let mut cfg = base();
        cfg.services[0].price = 0.1;
        assert_eq!(cfg.services[0].price_micros(), 100_000);
    }
}
