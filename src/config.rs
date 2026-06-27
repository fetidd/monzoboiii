use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub monzo: MonzoConfig,
}

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub secret: String,
    pub port: u16,
    pub redirect_uri: String,
}

#[derive(Deserialize, Clone)]
pub struct MonzoConfig {
    pub client_id: String,
    pub client_secret: String,
    pub pot_id: String,
    pub account_id: String,
    pub withdrawal_amount_pence: u64,
    pub trigger_types: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|_| anyhow::anyhow!("Could not read config.toml — copy config.toml.example and fill it in"))?;
        Ok(toml::from_str(&content)?)
    }
}

impl Tokens {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}
