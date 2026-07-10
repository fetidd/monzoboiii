pub mod accounts;
pub mod attachments;
pub mod balance;
pub mod common;
pub mod diagnose;
pub mod feed;
pub mod logout;
pub mod pots;
pub mod receipts;
pub mod transactions;
pub mod webhooks;
pub mod whoami;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "monzoctl", about = "CLI for the Monzo API and monzoboiii diagnostics")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Check config, tokens, and Monzo API connectivity
    Diagnose,
    /// GET /ping/whoami — info about the current access token
    Whoami,
    /// GET /accounts — list accounts owned by the authenticated user
    Accounts(accounts::AccountsArgs),
    /// GET /balance — balance information for an account
    Balance(balance::BalanceArgs),
    /// Pots: list, deposit into, and withdraw from pots
    Pots(pots::PotsArgs),
    /// Transactions: list, get, and annotate transactions
    Transactions(transactions::TransactionsArgs),
    /// POST /feed — create a feed item on the user's feed
    Feed(feed::FeedArgs),
    /// Attachments: upload, register, and deregister transaction attachments
    Attachments(attachments::AttachmentsArgs),
    /// Transaction receipts: create, get, and delete
    Receipts(receipts::ReceiptsArgs),
    /// Manage the Monzo webhook registered against this server
    Webhooks(webhooks::WebhooksArgs),
    /// POST /oauth2/logout — invalidate the current access token
    Logout,
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Command::Diagnose => diagnose::run().await,
            Command::Whoami => whoami::run().await,
            Command::Accounts(args) => accounts::run(args).await,
            Command::Balance(args) => balance::run(args).await,
            Command::Pots(args) => pots::run(args).await,
            Command::Transactions(args) => transactions::run(args).await,
            Command::Feed(args) => feed::run(args).await,
            Command::Attachments(args) => attachments::run(args).await,
            Command::Receipts(args) => receipts::run(args).await,
            Command::Webhooks(args) => webhooks::run(args).await,
            Command::Logout => logout::run().await,
        }
    }
}
