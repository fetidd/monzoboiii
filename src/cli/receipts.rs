use crate::cli::common::{self, BASE_URL, print_json};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ReceiptsArgs {
    #[command(subcommand)]
    pub command: ReceiptsCommand,
}

#[derive(Subcommand)]
pub enum ReceiptsCommand {
    /// Create or update a receipt from a JSON file matching Monzo's receipt schema
    /// (external_id, transaction_id, total, currency, items, and optionally taxes/payments/merchant)
    Create {
        /// Path to a JSON file containing the receipt body
        file: String,
    },
    /// Retrieve a receipt by its external id
    Get {
        /// External id the receipt was created with
        external_id: String,
    },
    /// Delete a receipt by its external id
    Delete {
        /// External id the receipt was created with
        external_id: String,
    },
}

pub async fn run(args: ReceiptsArgs) -> anyhow::Result<()> {
    match args.command {
        ReceiptsCommand::Create { file } => cmd_create(&file).await,
        ReceiptsCommand::Get { external_id } => cmd_get(&external_id).await,
        ReceiptsCommand::Delete { external_id } => cmd_delete(&external_id).await,
    }
}

async fn cmd_create(file: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let body: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;

    let res = http
        .put(format!("{BASE_URL}/transaction-receipts"))
        .bearer_auth(&tokens.access_token)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    println!("Receipt saved ({})", res.status());
    Ok(())
}

async fn cmd_get(external_id: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let res: serde_json::Value = http
        .get(format!("{BASE_URL}/transaction-receipts"))
        .bearer_auth(&tokens.access_token)
        .query(&[("external_id", external_id)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_delete(external_id: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    http.delete(format!("{BASE_URL}/transaction-receipts"))
        .bearer_auth(&tokens.access_token)
        .query(&[("external_id", external_id)])
        .send()
        .await?
        .error_for_status()?;

    println!("Deleted receipt: {external_id}");
    Ok(())
}
