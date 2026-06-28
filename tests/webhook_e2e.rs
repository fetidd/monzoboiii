use axum::{
    Json, Router,
    body::Body,
    extract::{Form, Path},
    http::{Request, StatusCode},
    routing::{get, post, put},
};
use monzoboiii::{
    build_app,
    config::{AppConfig, Config, MonzoConfig, Tokens},
    monzo::MonzoClient,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tower::ServiceExt;

fn test_config() -> Config {
    Config {
        app: AppConfig {
            secret: "test_secret".to_string(),
            port: 0,
            redirect_uri: "http://localhost/callback".to_string(),
        },
        monzo: MonzoConfig {
            client_id: "client_id".to_string(),
            client_secret: "client_secret".to_string(),
            account_id: "acc_123".to_string(),
        },
    }
}

/// Form body sent to PUT /pots/{pot_id}/withdraw.
#[derive(Deserialize)]
struct WithdrawForm {
    amount: u64,
}

/// Spawns a mock Monzo API server. GET /pots returns a single "groceries" pot.
/// PUT /pots/{pot_id}/withdraw records (pot_id, amount_pence) for each call.
async fn spawn_mock_monzo() -> (String, Arc<Mutex<Vec<(String, u64)>>>) {
    let calls: Arc<Mutex<Vec<(String, u64)>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = calls.clone();

    let mock_app = Router::new()
        .route(
            "/pots",
            get(|| async {
                Json(json!({
                    "pots": [
                        {"id": "pot_123", "name": "groceries", "deleted": false},
                        {"id": "pot_124", "name": "old_groceries", "deleted": true},
                        {"id": "pot_125", "name": "Eating out", "deleted": false},
                    ]
                }))
            }),
        )
        .route(
            "/pots/{pot_id}/withdraw",
            put(
                move |Path(pot_id): Path<String>, Form(form): Form<WithdrawForm>| {
                    let calls = calls_clone.clone();
                    async move {
                        calls.lock().await.push((pot_id, form.amount));
                        StatusCode::OK
                    }
                },
            ),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    (format!("http://127.0.0.1:{port}"), calls)
}

async fn build_test_app(mock_url: String) -> axum::Router {
    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), PathBuf::from("/dev/null"), test_config())
            .with_base_url(mock_url),
    );
    monzo.refresh_pot_map().await.unwrap();
    build_app(monzo)
}

fn webhook_request(secret: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/webhook/monzo/{secret}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn happy_path_triggers_pot_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": {"id": "tx_001", "account_id": "acc_123", "category": "eating_out", "amount": -500}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        calls.lock().await.as_slice(),
        [("pot_125".to_string(), 500u64)]
    );
}

#[tokio::test]
async fn bad_secret_returns_forbidden_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "wrong_secret",
            json!({
                "type": "transaction.created",
                "data": {"id": "tx_001", "account_id": "acc_123", "category": "groceries", "amount": -500}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn wrong_event_type_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.updated",
                "data": {"id": "tx_001", "account_id": "acc_123", "category": "groceries", "amount": -500}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn wrong_account_id_returns_forbidden_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": {"id": "tx_001", "account_id": "acc_other", "category": "groceries", "amount": -500}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn no_matching_pot_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    // "eating_out" is a valid spending category but no pot with that name exists in the mock
    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": {"id": "tx_001", "account_id": "acc_123", "category": "bad_category", "amount": -300}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn fifty_concurrent_transactions_all_handled() {
    let calls: Arc<Mutex<Vec<(String, u64)>>> = Arc::new(Mutex::new(Vec::new()));

    // The barrier requires all 50 withdrawal requests to be simultaneously
    // in-flight before any can complete. If the server handled requests one
    // at a time the barrier would never reach 50 and the test would deadlock.
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    {
        let calls = calls.clone();
        let barrier = barrier.clone();
        let mock_app = Router::new()
            .route(
                "/pots",
                get(|| async {
                    Json(json!({
                        "pots": [{"id": "pot_123", "name": "groceries", "deleted": false}]
                    }))
                }),
            )
            .route(
                "/pots/{pot_id}/withdraw",
                put(
                    move |Path(pot_id): Path<String>, Form(form): Form<WithdrawForm>| {
                        let calls = calls.clone();
                        let barrier = barrier.clone();
                        async move {
                            barrier.wait().await;
                            calls.lock().await.push((pot_id, form.amount));
                            StatusCode::OK
                        }
                    },
                ),
            );
        tokio::spawn(async move {
            axum::serve(mock_listener, mock_app).await.unwrap();
        });
    }

    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), PathBuf::from("/dev/null"), test_config())
            .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
    monzo.refresh_pot_map().await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, build_app(monzo)).await.unwrap();
    });

    let client = reqwest::Client::new();
    let mut set = tokio::task::JoinSet::new();
    for i in 0..50u32 {
        let client = client.clone();
        set.spawn(async move {
            client
                .post(format!("http://127.0.0.1:{port}/webhook/monzo/test_secret"))
                .json(&json!({
                    "type": "transaction.created",
                    "data": {"id": format!("tx_{i:03}"), "account_id": "acc_123", "category": "groceries", "amount": -100}
                }))
                .send()
                .await
                .unwrap()
        });
    }

    while let Some(result) = set.join_next().await {
        assert_eq!(result.unwrap().status(), StatusCode::OK);
    }

    assert_eq!(calls.lock().await.len(), 50);
}

#[tokio::test]
async fn expired_token_is_refreshed_and_withdrawal_retried() {
    let withdraw_attempts = Arc::new(AtomicU32::new(0));
    let refresh_calls = Arc::new(AtomicU32::new(0));

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    {
        let withdraw_attempts = withdraw_attempts.clone();
        let refresh_calls = refresh_calls.clone();
        let mock_app = Router::new()
            .route(
                "/pots",
                get(|| async {
                    Json(json!({
                        "pots": [{"id": "pot_123", "name": "groceries", "deleted": false}]
                    }))
                }),
            )
            .route(
                "/pots/{pot_id}/withdraw",
                put(move || {
                    let attempt = withdraw_attempts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        // First call simulates an expired token; second should succeed.
                        if attempt == 0 {
                            StatusCode::UNAUTHORIZED
                        } else {
                            StatusCode::OK
                        }
                    }
                }),
            )
            .route(
                "/oauth2/token",
                post(move || {
                    let refresh_calls = refresh_calls.clone();
                    async move {
                        refresh_calls.fetch_add(1, Ordering::SeqCst);
                        Json(json!({
                            "access_token": "refreshed_token",
                            "refresh_token": "new_refresh_token"
                        }))
                    }
                }),
            );
        tokio::spawn(async move {
            axum::serve(mock_listener, mock_app).await.unwrap();
        });
    }

    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), PathBuf::from("/dev/null"), test_config())
            .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
    monzo.refresh_pot_map().await.unwrap();
    let app = build_app(monzo);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": {"id": "tx_001", "account_id": "acc_123", "category": "groceries", "amount": -500}
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        withdraw_attempts.load(Ordering::SeqCst),
        2,
        "expected initial attempt + one retry"
    );
    assert_eq!(
        refresh_calls.load(Ordering::SeqCst),
        1,
        "expected exactly one token refresh"
    );
}
