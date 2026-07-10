pub mod diagnose;
pub mod webhooks;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "monzoctl", about = "Diagnostics and webhook management for monzoboiii")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Check config, tokens, and Monzo API connectivity
    Diagnose,
    /// Manage the Monzo webhook registered against this server
    Webhooks(webhooks::WebhooksArgs),
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Command::Diagnose => diagnose::run().await,
            Command::Webhooks(args) => webhooks::run(args).await,
        }
    }
}
