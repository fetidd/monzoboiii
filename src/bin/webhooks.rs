use monzoboiii::config::{Config, Tokens};
use reqwest::Client;
use std::path::Path;

const CONFIG_PATH: &str = "/home/ben/.config/monzoboiii/config.toml";
const TOKENS_PATH: &str = "tokens.toml";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list")   => cmd_list().await,
        Some("create") => match args.get(2) {
            Some(url) => cmd_create(url).await,
            None => {
                eprintln!("Usage: webhooks create <base_url>  (e.g. https://my-tunnel.example.com)");
                std::process::exit(1);
            }
        },
        Some("delete") => match args.get(2) {
            Some(id) => cmd_delete(id).await,
            None => {
                eprintln!("Usage: webhooks delete <webhook_id>");
                std::process::exit(1);
            }
        },
        Some("test") => cmd_test(args.get(2).map(String::as_str)).await,
        _ => {
            eprintln!("Usage: webhooks <list|create <url>|delete <id>|test [category]>");
            std::process::exit(1);
        }
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
        let id  = w["id"].as_str().unwrap_or("?");
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

    let id  = res["webhook"]["id"].as_str().unwrap_or("?");
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
    let config = monzoboiii::config::Config::load(Path::new(CONFIG_PATH))?;
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
