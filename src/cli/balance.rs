use crate::cli::common::{self, BASE_URL, print_json};
use clap::Args;

#[derive(Args)]
pub struct BalanceArgs {
    /// Account id to check (defaults to the account_id in config.toml)
    #[arg(long)]
    pub account_id: Option<String>,
}

pub async fn run(args: BalanceArgs) -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let account_id = args.account_id.unwrap_or(config.monzo.account_id);

    let res: serde_json::Value = http
        .get(format!("{BASE_URL}/balance"))
        .bearer_auth(&tokens.access_token)
        .query(&[("account_id", account_id.as_str())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}
