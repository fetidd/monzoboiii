use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::monzo::MonzoClient;

// Defines a struct and automatically appends a flattened `unknown` map that
// captures any fields Monzo sends that aren't in our typed definition yet.
macro_rules! monzo_struct {
    (
        $(#[$attr:meta])*
        pub struct $name:ident {
            $(
                $(#[$field_attr:meta])*
                pub $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        pub struct $name {
            $(
                $(#[$field_attr])*
                pub $field: $ty,
            )*
            #[serde(flatten)]
            pub unknown: HashMap<String, serde_json::Value>,
        }
    };
}

fn log_unknown_fields(unknown: &HashMap<String, serde_json::Value>, context: &str) {
    for (key, value) in unknown {
        let kind = match value {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "bool",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        };
        tracing::warn!("Unknown field in {context}: {key} ({kind}) = {value}");
    }
}

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
}

monzo_struct! {
    #[derive(Deserialize, Debug)]
    pub struct WebhookPayload {
        #[serde(rename = "type")]
        pub event_type: String,
        pub data: TransactionData
    }
}

monzo_struct! {
    #[derive(Deserialize, Debug)]
    pub struct TransactionData {
        pub id: String,
        pub account_id: String,
        pub amount: i64,
        pub amount_is_pending: bool,
        pub atm_fees_detailed: Option<serde_json::Value>,
        pub attachments: Option<serde_json::Value>,
        pub can_add_to_tab: bool,
        pub can_be_excluded_from_breakdown: bool,
        pub can_be_made_subscription: bool,
        pub can_match_transactions_in_categorization: bool,
        pub can_split_the_bill: bool,
        pub categories: Option<HashMap<String, i64>>,
        pub category: String,
        pub counterparty: serde_json::Value,
        pub created: String,
        pub currency: String,
        pub dedupe_id: String,
        pub description: String,
        pub fees: serde_json::Value,
        pub include_in_spending: bool,
        pub international: Option<serde_json::Value>,
        pub is_load: bool,
        pub labels: Option<Vec<String>>,
        pub local_amount: i64,
        pub local_currency: String,
        pub merchant: Option<Merchant>,
        pub merchant_feedback_uri: String,
        pub metadata: TransactionMetadata,
        pub notes: String,
        pub originator: bool,
        pub parent_account_id: String,
        pub scheme: String,
        #[serde(deserialize_with = "empty_string_as_none")]
        pub settled: Option<String>,
        pub updated: String,
        pub user_id: String
    }
}

monzo_struct! {
    #[derive(Deserialize, Debug)]
    pub struct Merchant {
        pub id: String,
        pub group_id: String,
        pub name: String,
        pub category: String,
        pub atm: bool,
        pub online: bool,
        pub disable_feedback: bool,
        pub emoji: String,
        pub logo: String,
        pub address: MerchantAddress,
        pub metadata: MerchantMetadata,
        #[serde(default)]
        pub suggested_tags: Option<String>
    }
}

monzo_struct! {
    #[derive(Deserialize, Debug)]
    pub struct MerchantAddress {
        pub address: String,
        pub approximate: bool,
        pub city: String,
        pub country: String,
        pub formatted: String,
        pub latitude: f64,
        pub longitude: f64,
        pub postcode: String,
        pub region: String,
        pub short_formatted: String,
        pub zoom_level: i32
    }
}

monzo_struct! {
    #[derive(Deserialize, Debug, Default)]
    #[serde(default)]
    pub struct MerchantMetadata {
        pub suggested_tags: Option<String>,
        pub website: Option<String>
    }
}

monzo_struct! {
    #[derive(Deserialize, Debug, Default)]
    #[serde(default)]
    pub struct TransactionMetadata {
        // Present on both schemes
        pub ledger_committed_timestamp_earliest: String,
        pub ledger_committed_timestamp_latest: String,
        pub ledger_insertion_id: String,
        pub transaction_description_localised: Option<String>,
        pub transaction_locale_country: Option<String>,

        // Mastercard — always present for that scheme
        pub eligible_for_pot_cover: Option<String>,
        pub mastercard_approval_type: Option<String>,
        pub mastercard_auth_message_id: Option<String>,
        pub mastercard_card_id: Option<String>,
        pub mastercard_lifecycle_id: Option<String>,
        pub mcc: Option<String>,
        pub standin_correlation_id: Option<String>,

        // Mastercard — joint account card (spender != account holder)
        pub spender_first_name: Option<String>,
        pub spender_full_name: Option<String>,

        // Mastercard — appears only after settlement
        pub mastercard_clearing_message_id: Option<String>,

        // Mastercard — tokenised card payments
        pub token_unique_reference: Option<String>,
        pub tokenization_method: Option<String>,

        // Mastercard — some merchants provide contact details
        pub card_acceptor_contact_number: Option<String>,
        pub card_acceptor_website: Option<String>,

        // Mastercard — round-up / coin jar linked transaction
        pub coin_jar_transaction: Option<String>,

        // uk_retail_pot — always present for that scheme
        pub external_id: Option<String>,
        pub money_transfer_id: Option<String>,
        pub move_money_transfer_id: Option<String>,
        pub pot_account_id: Option<String>,
        pub pot_id: Option<String>,
        pub pot_withdrawal_id: Option<String>,
        pub trigger: Option<String>,
        pub user_id: Option<String>,

        // uk_retail_pot — pot deposit events
        pub pot_deposit_id: Option<String>,
        pub triggered_by: Option<String>,

        // uk_retail_pot — absent on first event, present on subsequent updates
        pub hold_decision_duration: Option<String>,
        pub hold_decision_status: Option<String>,

        // Mastercard — contactless / tokenised tap-to-pay
        pub token_transaction_identifier: Option<String>,

        // Mastercard — retroactive pot cover applied after settlement
        pub retroactive_pot_id: Option<String>,

        // bacs direct debit
        pub bacs_direct_debit_instruction_id: Option<String>,
        pub bacs_payment_id: Option<String>,
        pub bacs_record_id: Option<String>,
        pub bills_pot_id: Option<String>,
        pub notes: Option<String>,
        pub subscription_id: Option<String>,

        // payport faster payments (outbound bank transfer)
        pub action_code: Option<String>,
        pub client_idempotency_key: Option<String>,
        pub coach_detected: Option<String>,
        pub confirmation_of_payee_decision_id: Option<String>,
        pub confirmation_of_payee_requester_id: Option<String>,
        pub device_fingerprint: Option<String>,
        pub duplicate_payment_prompt_enabled: Option<String>,
        pub faster_payment: Option<String>,
        pub faster_payment_initiator: Option<String>,
        #[serde(rename = "fps.trn")]
        pub fps_trn: Option<String>,
        pub fps_fpid: Option<String>,
        pub fps_payment_id: Option<String>,
        pub insertion: Option<String>,
        pub ip_address_attempt: Option<String>,
        pub notification_on_settle: Option<String>,
        pub outbound_payment_trace_id: Option<String>,
        pub payment_source: Option<String>,
        pub share_detected: Option<String>,
        pub trn: Option<String>,

        pub is_pot_to_pot_transfer: Option<String>,
        pub source_pot_id: Option<String>,
        pub destination_pot_id: Option<String>,
    }
}

pub async fn handle(
    Path(secret): Path<String>,
    State(monzo): State<Arc<MonzoClient>>,
    body: Bytes,
) -> StatusCode {
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse webhook body as JSON: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };
    tracing::debug!("Webhook payload: {}", raw);

    let payload: WebhookPayload = match serde_json::from_value(raw) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Webhook body missing expected fields: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    log_unknown_fields(&payload.unknown, "WebhookPayload");
    log_unknown_fields(&payload.data.unknown, "TransactionData");
    log_unknown_fields(&payload.data.metadata.unknown, "TransactionMetadata");
    if let Some(m) = &payload.data.merchant {
        log_unknown_fields(&m.unknown, "Merchant");
        log_unknown_fields(&m.address.unknown, "MerchantAddress");
        log_unknown_fields(&m.metadata.unknown, "MerchantMetadata");
    }

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

    let category = &payload.data.category;
    let amount = payload.data.amount;

    if let Some(eligible) = payload.data.metadata.eligible_for_pot_cover
        && eligible == "true"
        && payload.data.scheme == "mastercard"
        && amount < 0
    {
        match monzo
            .withdraw_for_category(category, amount.unsigned_abs(), &payload.data.id)
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
    } else {
        StatusCode::OK
    }
}
