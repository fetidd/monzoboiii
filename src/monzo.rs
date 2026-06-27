use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Context;

use crate::config::{Config, Tokens};

pub struct MonzoClient {
    tokens: Arc<RwLock<Tokens>>,
    tokens_path: PathBuf,
    pub config: Config,
    http: reqwest::Client,
    base_url: String,
}

impl MonzoClient {
    pub fn new(tokens: Tokens, tokens_path: PathBuf, config: Config) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(tokens)),
            tokens_path,
            config,
            http: reqwest::Client::new(),
            base_url: "https://api.monzo.com".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    pub async fn withdraw_from_pot(&self, dedupe_id: &str) -> anyhow::Result<()> {
        let token = self.tokens.read().await.access_token.clone();
        match self.try_withdraw(&token, dedupe_id).await {
            Err(e) if e.to_string().contains("401") => {
                tracing::info!("Access token expired, refreshing");
                let new_token = self.refresh().await?;
                self.try_withdraw(&new_token, dedupe_id).await
            }
            other => other,
        }
    }

    async fn try_withdraw(&self, token: &str, dedupe_id: &str) -> anyhow::Result<()> {
        let cfg = &self.config.monzo;
        let res = self.http
            .put(format!("{}/pots/{}/withdraw", self.base_url, cfg.pot_id))
            .bearer_auth(token)
            .form(&[
                ("destination_account_id", cfg.account_id.as_str()),
                ("amount", cfg.withdrawal_amount_pence.to_string().as_str()),
                ("dedupe_id", dedupe_id),
            ])
            .send()
            .await?;

        if res.status() == 401 {
            anyhow::bail!("401 Unauthorized");
        }
        res.error_for_status().context("Monzo pot withdrawal failed")?;
        Ok(())
    }

    pub async fn refresh(&self) -> anyhow::Result<String> {
        let refresh_token = self.tokens.read().await.refresh_token.clone();
        let cfg = &self.config.monzo;

        let res: serde_json::Value = self.http
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
            access_token: res["access_token"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing access_token in refresh response — re-auth needed at /auth/reauth"))?
                .to_string(),
            refresh_token: res["refresh_token"].as_str()
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
        let res: serde_json::Value = self.http
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
            access_token: res["access_token"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing access_token in exchange response"))?
                .to_string(),
            refresh_token: res["refresh_token"].as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing refresh_token in exchange response"))?
                .to_string(),
        };

        new_tokens.save(&self.tokens_path)?;
        *self.tokens.write().await = new_tokens;
        tracing::info!("New tokens saved after OAuth exchange");
        Ok(())
    }
}
