use crate::cli::common::{self, BASE_URL};
use clap::Args;

#[derive(Args)]
pub struct FeedArgs {
    /// Feed item title
    #[arg(long)]
    pub title: String,
    /// Image URL shown alongside the feed item
    #[arg(long)]
    pub image_url: String,
    /// Body text
    #[arg(long)]
    pub body: Option<String>,
    /// URL opened when the feed item is tapped
    #[arg(long)]
    pub url: Option<String>,
    /// Background color, hex format #RRGGBB
    #[arg(long)]
    pub background_color: Option<String>,
    /// Title color, hex format #RRGGBB
    #[arg(long)]
    pub title_color: Option<String>,
    /// Body color, hex format #RRGGBB
    #[arg(long)]
    pub body_color: Option<String>,
    /// Account id (defaults to the account_id in config.toml)
    #[arg(long)]
    pub account_id: Option<String>,
}

pub async fn run(args: FeedArgs) -> anyhow::Result<()> {
    let (config, tokens, http) = common::load().await?;
    let account_id = args.account_id.unwrap_or(config.monzo.account_id);

    let mut form: Vec<(String, String)> = vec![
        ("account_id".into(), account_id),
        ("type".into(), "basic".into()),
        ("params[title]".into(), args.title),
        ("params[image_url]".into(), args.image_url),
    ];
    if let Some(url) = args.url {
        form.push(("url".into(), url));
    }
    if let Some(body) = args.body {
        form.push(("params[body]".into(), body));
    }
    if let Some(c) = args.background_color {
        form.push(("params[background_color]".into(), c));
    }
    if let Some(c) = args.title_color {
        form.push(("params[title_color]".into(), c));
    }
    if let Some(c) = args.body_color {
        form.push(("params[body_color]".into(), c));
    }

    let status = http
        .post(format!("{BASE_URL}/feed"))
        .bearer_auth(&tokens.access_token)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .status();

    println!("Feed item created ({status})");
    Ok(())
}
