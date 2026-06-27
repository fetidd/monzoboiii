use axum::{
    Json, Router,
    body::Body,
    extract::Path,
    http::{Request, StatusCode},
    routing::{post, put},
};
use std::sync::atomic::{AtomicU32, Ordering};
use monzoboiii::{
    build_app,
    config::{AppConfig, Config, MonzoConfig, Tokens},
    monzo::MonzoClient,
};
use serde_json::json;
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
            pot_id: "pot_123".to_string(),
            account_id: "acc_123".to_string(),
            withdrawal_amount_pence: 100,
            trigger_types: vec!["card".to_string()],
        },
    }
}

/// Spawns a mock Monzo API server and returns its base URL along with a list
/// that accumulates the pot IDs received in PUT /pots/{pot_id}/withdraw calls.
async fn spawn_mock_monzo() -> (String, Arc<Mutex<Vec<String>>>) {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let calls_clone = calls.clone();

    let mock_app = Router::new().route(
        "/pots/{pot_id}/withdraw",
        put(move |Path(pot_id): Path<String>| {
            let calls = calls_clone.clone();
            async move {
                calls.lock().await.push(pot_id);
                StatusCode::OK
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    (format!("http://127.0.0.1:{port}"), calls)
}

fn build_test_app(mock_url: String) -> axum::Router {
    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), PathBuf::from("/dev/null"), test_config())
            .with_base_url(mock_url),
    );
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
    let app = build_test_app(mock_url);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": { "id": "tx_001", "account_id": "acc_123", "type": "card" }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(calls.lock().await.as_slice(), ["pot_123"]);
}

#[tokio::test]
async fn bad_secret_returns_forbidden_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url);

    let res = app
        .oneshot(webhook_request(
            "wrong_secret",
            json!({
                "type": "transaction.created",
                "data": { "id": "tx_001", "account_id": "acc_123", "type": "card" }
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
    let app = build_test_app(mock_url);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.updated",
                "data": { "id": "tx_001", "account_id": "acc_123", "type": "card" }
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
    let app = build_test_app(mock_url);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": { "id": "tx_001", "account_id": "acc_other", "type": "card" }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn fifty_concurrent_transactions_all_handled() {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // The barrier requires all 50 withdrawal requests to be simultaneously
    // in-flight before any can complete. If the server handled requests one
    // at a time the barrier would never reach 50 and the test would deadlock.
    let barrier = Arc::new(tokio::sync::Barrier::new(50));

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    {
        let calls = calls.clone();
        let barrier = barrier.clone();
        let mock_app = Router::new().route(
            "/pots/{pot_id}/withdraw",
            put(move |Path(pot_id): Path<String>| {
                let calls = calls.clone();
                let barrier = barrier.clone();
                async move {
                    barrier.wait().await;
                    calls.lock().await.push(pot_id);
                    StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(mock_listener, mock_app).await.unwrap();
        });
    }

    let monzo = Arc::new(
        MonzoClient::new(
            Tokens::default(),
            PathBuf::from("/dev/null"),
            test_config(),
        )
        .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
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
                    "data": { "id": format!("tx_{i:03}"), "account_id": "acc_123", "type": "card" }
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
                "/pots/{pot_id}/withdraw",
                put(move || {
                    let attempt = withdraw_attempts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        // First call simulates an expired token; second should succeed.
                        if attempt == 0 { StatusCode::UNAUTHORIZED } else { StatusCode::OK }
                    }
                }),
            )
            .route(
                "/oauth2/token",
                post(move || {
                    let refresh_calls = refresh_calls.clone();
                    async move {
                        refresh_calls.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({
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
    let app = build_app(monzo);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": { "id": "tx_001", "account_id": "acc_123", "type": "card" }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(withdraw_attempts.load(Ordering::SeqCst), 2, "expected initial attempt + one retry");
    assert_eq!(refresh_calls.load(Ordering::SeqCst), 1, "expected exactly one token refresh");
}

#[tokio::test]
async fn non_trigger_type_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url);

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": { "id": "tx_001", "account_id": "acc_123", "type": "pot_transfer" }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}
