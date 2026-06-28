use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::monzo::MonzoClient;

#[derive(Deserialize, Debug)]
pub struct WebhookPayload {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: TransactionData,
}

#[derive(Deserialize, Debug)]
pub struct TransactionData {
    pub id: String,
    pub account_id: String,
    pub amount: Option<i64>,
    pub category: Option<String>,
}

pub async fn handle(
    Path(secret): Path<String>,
    State(monzo): State<Arc<MonzoClient>>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    tracing::info!("Received {:?}", payload);
    if secret != monzo.config.app.secret {
        tracing::warn!("Webhook rejected: bad secret");
        return StatusCode::FORBIDDEN;
    }

    if payload.event_type != "transaction.created" {
        return StatusCode::OK;
    }

    if payload.data.account_id != monzo.config.monzo.account_id {
        tracing::warn!(
            "Webhook for unrecognised account: {}",
            payload.data.account_id
        );
        return StatusCode::FORBIDDEN;
    }

    let category = payload.data.category.as_deref().unwrap_or("");
    let amount = payload.data.amount.unwrap_or(0).unsigned_abs();

    match monzo
        .withdraw_for_category(category, amount, &payload.data.id)
        .await
    {
        Ok(true) => {
            tracing::info!(
                "Pot withdrawal triggered for category '{category}' tx {}",
                payload.data.id
            );
            StatusCode::OK
        }
        Ok(false) => StatusCode::OK,
        Err(e) => {
            tracing::error!("Pot withdrawal failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
