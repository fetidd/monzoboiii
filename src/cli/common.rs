use crate::config::{Config, Tokens};
use reqwest::Client;
use std::path::Path;

pub const CONFIG_PATH: &str = "/home/ben/.config/monzoboiii/config.toml";
pub const TOKENS_PATH: &str = "tokens.toml";
pub const BASE_URL: &str = "https://api.monzo.com";

pub async fn load() -> anyhow::Result<(Config, Tokens, Client)> {
    let config = Config::load(Path::new(CONFIG_PATH))?;
    let tokens = Tokens::load(Path::new(TOKENS_PATH)).unwrap_or_default();
    if tokens.access_token.is_empty() {
        anyhow::bail!(
            "Not authenticated — start the server and visit http://localhost:{}/auth/reauth",
            config.app.port
        );
    }
    Ok((config, tokens, Client::new()))
}

pub fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}
