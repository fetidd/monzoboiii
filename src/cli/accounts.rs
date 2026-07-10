use crate::cli::common::{self, BASE_URL, print_json};
use clap::Args;

#[derive(Args)]
pub struct AccountsArgs {
    /// Filter by account type, e.g. uk_retail or uk_retail_joint
    #[arg(long)]
    pub account_type: Option<String>,
}

pub async fn run(args: AccountsArgs) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let mut req = http
        .get(format!("{BASE_URL}/accounts"))
        .bearer_auth(&tokens.access_token);
    if let Some(account_type) = &args.account_type {
        req = req.query(&[("account_type", account_type.as_str())]);
    }
    let res: serde_json::Value = req.send().await?.error_for_status()?.json().await?;

    print_json(&res);
    Ok(())
}
