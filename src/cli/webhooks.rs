use crate::config::{Config, Tokens};
use clap::{Args, Subcommand};
use reqwest::Client;
use std::path::Path;

const CONFIG_PATH: &str = "/home/ben/.config/monzoboiii/config.toml";
const TOKENS_PATH: &str = "tokens.toml";

#[derive(Args)]
pub struct WebhooksArgs {
    #[command(subcommand)]
    pub command: WebhooksCommand,
}

#[derive(Subcommand)]
pub enum WebhooksCommand {
    /// List webhooks registered on the configured account
    List,
    /// Register a new webhook pointing at <base_url>/webhook/monzo/<secret>
    Create {
        /// Public base URL of this server, e.g. https://my-tunnel.example.com
        base_url: String,
    },
    /// Delete a webhook by id
    Delete {
        /// Webhook id, e.g. webhook_0000000000000000000000
        id: String,
    },
    /// Send a test transaction.created event to the local server
    Test {
        /// Spending category to use in the test payload (default: eating_out)
        category: Option<String>,
    },
}

pub async fn run(args: WebhooksArgs) -> anyhow::Result<()> {
    match args.command {
        WebhooksCommand::List => cmd_list().await,
        WebhooksCommand::Create { base_url } => cmd_create(&base_url).await,
        WebhooksCommand::Delete { id } => cmd_delete(&id).await,
        WebhooksCommand::Test { category } => cmd_test(category.as_deref()).await,
    }
}

async fn load() -> anyhow::Result<(Config, Tokens, Client)> {
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

async fn cmd_list() -> anyhow::Result<()> {
    let (config, tokens, http) = load().await?;
    let res: serde_json::Value = http
        .get("https://api.monzo.com/webhooks")
        .bearer_auth(&tokens.access_token)
        .query(&[("account_id", config.monzo.account_id.as_str())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let webhooks = res["webhooks"].as_array().map(Vec::as_slice).unwrap_or_default();
    if webhooks.is_empty() {
        println!("No webhooks registered.");
        return Ok(());
    }
    for w in webhooks {
        let id = w["id"].as_str().unwrap_or("?");
        let url = w["url"].as_str().unwrap_or("?");
        println!("{id}  {url}");
    }
    Ok(())
}

async fn cmd_create(base_url: &str) -> anyhow::Result<()> {
    let (config, tokens, http) = load().await?;
    let url = format!(
        "{}/webhook/monzo/{}",
        base_url.trim_end_matches('/'),
        config.app.secret,
    );
    let res: serde_json::Value = http
        .post("https://api.monzo.com/webhooks")
        .bearer_auth(&tokens.access_token)
        .form(&[
            ("account_id", config.monzo.account_id.as_str()),
            ("url", url.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let id = res["webhook"]["id"].as_str().unwrap_or("?");
    let url = res["webhook"]["url"].as_str().unwrap_or("?");
    println!("Created: {id}  {url}");
    Ok(())
}

async fn cmd_delete(id: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = load().await?;
    http.delete(format!("https://api.monzo.com/webhooks/{id}"))
        .bearer_auth(&tokens.access_token)
        .send()
        .await?
        .error_for_status()?;

    println!("Deleted: {id}");
    Ok(())
}

async fn cmd_test(category: Option<&str>) -> anyhow::Result<()> {
    let config = Config::load(Path::new(CONFIG_PATH))?;
    let http = Client::new();

    let url = format!(
        "http://localhost:{}/webhook/monzo/{}",
        config.app.port, config.app.secret,
    );

    let payload = serde_json::json!({
        "type": "transaction.created",
        "data": {
            "id": "tx_test_000000000000",
            "account_id": config.monzo.account_id,
            "amount": -500,
            "category": category.unwrap_or("eating_out"),
        }
    });

    let status = http.post(&url).json(&payload).send().await?.status();
    println!("POST {url}  →  {status}");
    Ok(())
}
