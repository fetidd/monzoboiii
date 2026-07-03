use monzoboiii::{build_app, config, monzo};
use std::{path::Path, sync::Arc};
use tokio::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::Config::load(Path::new("/home/ben/.config/monzoboiii/config.toml"))?;
    let tokens = config::Tokens::load(Path::new("tokens.toml")).unwrap_or_default();
    let port = config.app.port;

    let monzo = Arc::new(monzo::MonzoClient::new(
        tokens,
        Path::new("tokens.toml").to_path_buf(),
        config,
    ));

    if let Err(e) = monzo.refresh_pot_map().await {
        tracing::warn!("Initial pot map refresh failed (not yet authenticated?): {e}");
    }

    {
        let monzo = monzo.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                if let Err(e) = monzo.refresh_pot_map().await {
                    tracing::error!("Pot map refresh failed: {e}");
                }
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("Listening on :{port}");
    axum::serve(listener, build_app(monzo)).await?;
    Ok(())
}
