use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::monzo::MonzoClient;

#[derive(Deserialize)]
pub struct WebhookPayload {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: TransactionData,
}

#[derive(Deserialize)]
pub struct TransactionData {
    pub id: String,
    pub account_id: String,
    #[serde(rename = "type")]
    pub transaction_type: Option<String>,
}

pub async fn handle(
    Path(secret): Path<String>,
    State(monzo): State<Arc<MonzoClient>>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    if secret != monzo.config.app.secret {
        tracing::warn!("Webhook rejected: bad secret");
        return StatusCode::FORBIDDEN;
    }

    if payload.event_type != "transaction.created" {
        return StatusCode::OK;
    }

    if payload.data.account_id != monzo.config.monzo.account_id {
        tracing::warn!("Webhook for unrecognised account: {}", payload.data.account_id);
        return StatusCode::FORBIDDEN;
    }

    let tx_type = payload.data.transaction_type.as_deref().unwrap_or("");
    if !monzo.config.monzo.trigger_types.iter().any(|t| t == tx_type) {
        return StatusCode::OK;
    }

    match monzo.withdraw_from_pot(&payload.data.id).await {
        Ok(_) => {
            tracing::info!("Pot withdrawal triggered by transaction {}", payload.data.id);
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!("Pot withdrawal failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
