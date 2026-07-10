use crate::cli::common::{self, BASE_URL, print_json};

pub async fn run() -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let res: serde_json::Value = http
        .get(format!("{BASE_URL}/ping/whoami"))
        .bearer_auth(&tokens.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}
