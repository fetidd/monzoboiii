use crate::cli::common::{self, BASE_URL, print_json};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TransactionsArgs {
    #[command(subcommand)]
    pub command: TransactionsCommand,
}

#[derive(Subcommand)]
pub enum TransactionsCommand {
    /// List transactions on the configured account
    List {
        /// Account id (defaults to the account_id in config.toml)
        #[arg(long)]
        account_id: Option<String>,
        /// Only return transactions created after this time (RFC 3339) or transaction id
        #[arg(long)]
        since: Option<String>,
        /// Only return transactions created before this time (RFC 3339)
        #[arg(long)]
        before: Option<String>,
        /// Maximum number of transactions to return
        #[arg(long)]
        limit: Option<u32>,
        /// Expand the merchant object inline
        #[arg(long)]
        expand_merchant: bool,
    },
    /// Retrieve a single transaction by id
    Get {
        /// Transaction id, e.g. tx_0000000000000000000000
        transaction_id: String,
        /// Expand the merchant object inline
        #[arg(long)]
        expand_merchant: bool,
    },
    /// Add, update, or delete metadata annotations on a transaction
    Annotate {
        /// Transaction id, e.g. tx_0000000000000000000000
        transaction_id: String,
        /// key=value pair to set; use key= (empty value) to delete a key. Repeatable.
        #[arg(short = 'm', long = "metadata", value_name = "KEY=VALUE", required = true)]
        metadata: Vec<String>,
    },
}

pub async fn run(args: TransactionsArgs) -> anyhow::Result<()> {
    match args.command {
        TransactionsCommand::List {
            account_id,
            since,
            before,
            limit,
            expand_merchant,
        } => cmd_list(account_id, since, before, limit, expand_merchant).await,
        TransactionsCommand::Get {
            transaction_id,
            expand_merchant,
        } => cmd_get(&transaction_id, expand_merchant).await,
        TransactionsCommand::Annotate {
            transaction_id,
            metadata,
        } => cmd_annotate(&transaction_id, metadata).await,
    }
}

async fn cmd_list(
    account_id: Option<String>,
    since: Option<String>,
    before: Option<String>,
    limit: Option<u32>,
    expand_merchant: bool,
) -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let account_id = account_id.unwrap_or(config.monzo.account_id);

    let mut query: Vec<(String, String)> = vec![("account_id".into(), account_id)];
    if let Some(since) = since {
        query.push(("since".into(), since));
    }
    if let Some(before) = before {
        query.push(("before".into(), before));
    }
    if let Some(limit) = limit {
        query.push(("limit".into(), limit.to_string()));
    }
    if expand_merchant {
        query.push(("expand[]".into(), "merchant".into()));
    }

    let res: serde_json::Value = http
        .get(format!("{BASE_URL}/transactions"))
        .bearer_auth(&tokens.access_token)
        .query(&query)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_get(transaction_id: &str, expand_merchant: bool) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let mut req = http
        .get(format!("{BASE_URL}/transactions/{transaction_id}"))
        .bearer_auth(&tokens.access_token);
    if expand_merchant {
        req = req.query(&[("expand[]", "merchant")]);
    }

    let res: serde_json::Value = req.send().await?.error_for_status()?.json().await?;

    print_json(&res);
    Ok(())
}

async fn cmd_annotate(transaction_id: &str, metadata: Vec<String>) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;

    let form: Vec<(String, String)> = metadata
        .iter()
        .map(|kv| {
            let (key, value) = kv.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("Invalid metadata '{kv}' — expected KEY=VALUE")
            })?;
            Ok((format!("metadata[{key}]"), value.to_string()))
        })
        .collect::<anyhow::Result<_>>()?;

    let res: serde_json::Value = http
        .patch(format!("{BASE_URL}/transactions/{transaction_id}"))
        .bearer_auth(&tokens.access_token)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}
