use crate::cli::common::{self, BASE_URL, print_json};
use clap::{Args, Subcommand};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Args)]
pub struct PotsArgs {
    #[command(subcommand)]
    pub command: PotsCommand,
}

#[derive(Subcommand)]
pub enum PotsCommand {
    /// List pots for the configured account
    List,
    /// Move money from the account into a pot
    Deposit {
        /// Pot id, e.g. pot_0000000000000000000000
        pot_id: String,
        /// Amount in minor units (e.g. pence for GBP)
        amount: u64,
        /// Idempotency key; auto-generated if omitted
        #[arg(long)]
        dedupe_id: Option<String>,
        /// Source account id (defaults to the account_id in config.toml)
        #[arg(long)]
        source_account_id: Option<String>,
    },
    /// Move money from a pot into the account
    Withdraw {
        /// Pot id, e.g. pot_0000000000000000000000
        pot_id: String,
        /// Amount in minor units (e.g. pence for GBP)
        amount: u64,
        /// Idempotency key; auto-generated if omitted
        #[arg(long)]
        dedupe_id: Option<String>,
        /// Destination account id (defaults to the account_id in config.toml)
        #[arg(long)]
        destination_account_id: Option<String>,
    },
}

pub async fn run(args: PotsArgs) -> anyhow::Result<()> {
    match args.command {
        PotsCommand::List => cmd_list().await,
        PotsCommand::Deposit {
            pot_id,
            amount,
            dedupe_id,
            source_account_id,
        } => cmd_deposit(&pot_id, amount, dedupe_id, source_account_id).await,
        PotsCommand::Withdraw {
            pot_id,
            amount,
            dedupe_id,
            destination_account_id,
        } => cmd_withdraw(&pot_id, amount, dedupe_id, destination_account_id).await,
    }
}

fn generate_dedupe_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("monzoctl-{nanos}")
}

async fn cmd_list() -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let res: serde_json::Value = http
        .get(format!("{BASE_URL}/pots"))
        .bearer_auth(&tokens.access_token)
        .query(&[("current_account_id", config.monzo.account_id.as_str())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_deposit(
    pot_id: &str,
    amount: u64,
    dedupe_id: Option<String>,
    source_account_id: Option<String>,
) -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let source_account_id = source_account_id.unwrap_or(config.monzo.account_id);
    let dedupe_id = dedupe_id.unwrap_or_else(generate_dedupe_id);
    let amount = amount.to_string();

    let res: serde_json::Value = http
        .put(format!("{BASE_URL}/pots/{pot_id}/deposit"))
        .bearer_auth(&tokens.access_token)
        .form(&[
            ("source_account_id", source_account_id.as_str()),
            ("amount", amount.as_str()),
            ("dedupe_id", dedupe_id.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_withdraw(
    pot_id: &str,
    amount: u64,
    dedupe_id: Option<String>,
    destination_account_id: Option<String>,
) -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let destination_account_id = destination_account_id.unwrap_or(config.monzo.account_id);
    let dedupe_id = dedupe_id.unwrap_or_else(generate_dedupe_id);
    let amount = amount.to_string();

    let res: serde_json::Value = http
        .put(format!("{BASE_URL}/pots/{pot_id}/withdraw"))
        .bearer_auth(&tokens.access_token)
        .form(&[
            ("destination_account_id", destination_account_id.as_str()),
            ("amount", amount.as_str()),
            ("dedupe_id", dedupe_id.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}
