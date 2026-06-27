use monzoboiii::{build_app, config, monzo};
use std::{path::Path, sync::Arc};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::Config::load(Path::new("config.toml"))?;
    let tokens = config::Tokens::load(Path::new("tokens.toml")).unwrap_or_default();
    let port = config.app.port;

    let monzo = Arc::new(monzo::MonzoClient::new(
        tokens,
        Path::new("tokens.toml").to_path_buf(),
        config,
    ));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("Listening on :{port}");
    axum::serve(listener, build_app(monzo)).await?;
    Ok(())
}
