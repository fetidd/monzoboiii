use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{Config, Tokens};

const SPENDING_CATEGORIES: &[&str] = &[
    "general",
    "eating_out",
    "expenses",
    "transport",
    "cash",
    // "bills",
    "entertainment",
    "shopping",
    "holidays",
    "groceries",
    "family",
    "charity",
    "personal_care",
    // "savings",
];

#[derive(Deserialize)]
struct Pot {
    id: String,
    name: String,
    deleted: bool,
}

#[derive(Deserialize)]
struct PotsResponse {
    pots: Vec<Pot>,
}

pub struct MonzoClient {
    tokens: Arc<RwLock<Tokens>>,
    tokens_path: PathBuf,
    pub config: Config,
    http: reqwest::Client,
    base_url: String,
    pot_map: Arc<RwLock<HashMap<String, String>>>,
}

impl MonzoClient {
    pub fn new(tokens: Tokens, tokens_path: PathBuf, config: Config) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(tokens)),
            tokens_path,
            config,
            http: reqwest::Client::new(),
            base_url: "https://api.monzo.com".to_string(),
            pot_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    pub async fn refresh_pot_map(&self) -> anyhow::Result<()> {
        let token = self.tokens.read().await.access_token.clone();
        let res: PotsResponse = self
            .http
            .get(format!("{}/pots", self.base_url))
            .bearer_auth(&token)
            .query(&[("current_account_id", self.config.monzo.account_id.as_str())])
            .send()
            .await?
            .json()
            .await?;

        let mut map = HashMap::new();
        for pot in res.pots {
            let pot_category = pot.name.replace(" ", "_").to_lowercase();
            if !pot.deleted && SPENDING_CATEGORIES.contains(&pot_category.as_str()) {
                map.insert(pot_category, pot.id);
            }
        }
        let count = map.len();
        *self.pot_map.write().await = map;
        tracing::info!("Pot map refreshed: {count} spending pots found");
        Ok(())
    }

    pub async fn withdraw_for_category(
        &self,
        category: &str,
        amount: u64,
        dedupe_id: &str,
    ) -> anyhow::Result<bool> {
        let pot_id = {
            let map = self.pot_map.read().await;
            match map.get(category) {
                Some(id) => id.clone(),
                None => return Ok(false),
            }
        };
        tracing::info!("would move {amount} due to {category}");
        return Ok(false);
        // let token = self.tokens.read().await.access_token.clone();
        // match self.try_withdraw(&token, &pot_id, amount, dedupe_id).await {
        //     Err(e) if e.to_string().contains("401") => {
        //         tracing::info!("Access token expired, refreshing");
        //         let new_token = self.refresh().await?;
        //         self.try_withdraw(&new_token, &pot_id, amount, dedupe_id)
        //             .await?;
        //     }
        //     other => {
        //         other?;
        //     }
        // }
        // Ok(true)
    }

    async fn try_withdraw(
        &self,
        token: &str,
        pot_id: &str,
        amount: u64,
        dedupe_id: &str,
    ) -> anyhow::Result<()> {
        let res = self
            .http
            .put(format!("{}/pots/{}/withdraw", self.base_url, pot_id))
            .bearer_auth(token)
            .form(&[
                (
                    "destination_account_id",
                    self.config.monzo.account_id.as_str(),
                ),
                ("amount", amount.to_string().as_str()),
                ("dedupe_id", dedupe_id),
            ])
            .send()
            .await?;

        if res.status() == 401 {
            anyhow::bail!("401 Unauthorized");
        }
        res.error_for_status()
            .context("Monzo pot withdrawal failed")?;
        Ok(())
    }

    pub async fn refresh(&self) -> anyhow::Result<String> {
        let refresh_token = self.tokens.read().await.refresh_token.clone();
        let cfg = &self.config.monzo;

        let res: serde_json::Value = self
            .http
            .post(format!("{}/oauth2/token", self.base_url))
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", cfg.client_id.as_str()),
                ("client_secret", cfg.client_secret.as_str()),
                ("refresh_token", refresh_token.as_str()),
            ])
            .send()
            .await?
            .json()
            .await?;

        let new_tokens = Tokens {
            access_token: res["access_token"]
                .as_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing access_token in refresh response — re-auth needed at /auth/reauth"
                    )
                })?
                .to_string(),
            refresh_token: res["refresh_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing refresh_token in refresh response"))?
                .to_string(),
        };

        let access = new_tokens.access_token.clone();
        new_tokens.save(&self.tokens_path)?;
        *self.tokens.write().await = new_tokens;
        tracing::info!("Tokens refreshed and persisted to disk");
        Ok(access)
    }

    pub async fn exchange_code(&self, code: &str) -> anyhow::Result<()> {
        let cfg = &self.config.monzo;
        let res: serde_json::Value = self
            .http
            .post(format!("{}/oauth2/token", self.base_url))
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", cfg.client_id.as_str()),
                ("client_secret", cfg.client_secret.as_str()),
                ("redirect_uri", self.config.app.redirect_uri.as_str()),
                ("code", code),
            ])
            .send()
            .await?
            .json()
            .await?;

        let new_tokens = Tokens {
            access_token: res["access_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing access_token in exchange response"))?
                .to_string(),
            refresh_token: res["refresh_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing refresh_token in exchange response"))?
                .to_string(),
        };

        new_tokens.save(&self.tokens_path)?;
        *self.tokens.write().await = new_tokens;
        tracing::info!("New tokens saved after OAuth exchange");
        Ok(())
    }
}
