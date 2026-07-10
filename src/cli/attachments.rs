use crate::cli::common::{self, BASE_URL, print_json};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AttachmentsArgs {
    #[command(subcommand)]
    pub command: AttachmentsCommand,
}

#[derive(Subcommand)]
pub enum AttachmentsCommand {
    /// Obtain a temporary URL for uploading an attachment file
    Upload {
        /// File name, e.g. receipt.jpg
        file_name: String,
        /// MIME type, e.g. image/jpeg
        file_type: String,
        /// Content length in bytes
        content_length: u64,
    },
    /// Register an uploaded (or externally hosted) attachment against a transaction
    Register {
        /// Transaction id to attach to
        transaction_id: String,
        /// Publicly accessible URL of the file
        file_url: String,
        /// MIME type, e.g. image/jpeg
        file_type: String,
    },
    /// Remove an attachment
    Deregister {
        /// Attachment id, e.g. attach_0000000000000000000000
        id: String,
    },
}

pub async fn run(args: AttachmentsArgs) -> anyhow::Result<()> {
    match args.command {
        AttachmentsCommand::Upload {
            file_name,
            file_type,
            content_length,
        } => cmd_upload(&file_name, &file_type, content_length).await,
        AttachmentsCommand::Register {
            transaction_id,
            file_url,
            file_type,
        } => cmd_register(&transaction_id, &file_url, &file_type).await,
        AttachmentsCommand::Deregister { id } => cmd_deregister(&id).await,
    }
}

async fn cmd_upload(file_name: &str, file_type: &str, content_length: u64) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;
    let content_length = content_length.to_string();

    let res: serde_json::Value = http
        .post(format!("{BASE_URL}/attachment/upload"))
        .bearer_auth(&tokens.access_token)
        .form(&[
            ("file_name", file_name),
            ("file_type", file_type),
            ("content_length", content_length.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_register(transaction_id: &str, file_url: &str, file_type: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;

    let res: serde_json::Value = http
        .post(format!("{BASE_URL}/attachment/register"))
        .bearer_auth(&tokens.access_token)
        .form(&[
            ("external_id", transaction_id),
            ("file_url", file_url),
            ("file_type", file_type),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    print_json(&res);
    Ok(())
}

async fn cmd_deregister(id: &str) -> anyhow::Result<()> {
    let (_, tokens, http) = common::load().await?;

    http.post(format!("{BASE_URL}/attachment/deregister"))
        .bearer_auth(&tokens.access_token)
        .form(&[("id", id)])
        .send()
        .await?
        .error_for_status()?;

    println!("Deregistered: {id}");
    Ok(())
}
