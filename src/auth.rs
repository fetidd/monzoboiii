use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::monzo::MonzoClient;

pub async fn reauth(State(monzo): State<Arc<MonzoClient>>) -> impl IntoResponse {
    let cfg = &monzo.config;
    let url = format!(
        "https://auth.monzo.com/?client_id={}&redirect_uri={}&response_type=code&state={}",
        cfg.monzo.client_id,
        urlencoding::encode(&cfg.app.redirect_uri),
        "6dbd8dnd9dj"
    );
    Redirect::temporary(&url)
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
}

pub async fn callback(
    State(monzo): State<Arc<MonzoClient>>,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    match monzo.exchange_code(&params.code).await {
        Ok(_) => (StatusCode::OK, "Authenticated — you can close this tab.").into_response(),
        Err(e) => {
            tracing::error!("OAuth exchange failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Auth failed — check logs.",
            )
                .into_response()
        }
    }
}
