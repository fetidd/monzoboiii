use crate::cli::common::{self, BASE_URL, TOKENS_PATH};
use crate::config::Tokens;
use std::path::Path;

pub async fn run() -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;

    http.post(format!("{BASE_URL}/oauth2/logout"))
        .bearer_auth(&tokens.access_token)
        .send()
        .await?
        .error_for_status()?;

    Tokens::default().save(Path::new(TOKENS_PATH))?;
    println!("Logged out and cleared local tokens.");
    Ok(())
}
