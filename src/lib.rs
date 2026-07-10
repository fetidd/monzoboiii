pub mod auth;
pub mod cli;
pub mod config;
pub mod monzo;
pub mod webhook;

use axum::{Router, routing::{get, post}};
use std::sync::Arc;

pub fn build_app(monzo: Arc<monzo::MonzoClient>) -> Router {
    Router::new()
        .route("/webhook/monzo/{secret}", post(webhook::handle))
        .route("/auth/reauth", get(auth::reauth))
        .route("/auth/callback", get(auth::callback))
        .with_state(monzo)
}
