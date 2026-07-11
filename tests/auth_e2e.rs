use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use monzoboiii::{
    build_app,
    config::{AppConfig, Config, MonzoConfig, Tokens},
    monzo::MonzoClient,
};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{path::PathBuf, sync::Arc};
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

/// A unique per-test file path so tests can run in parallel without
/// clobbering each other's saved tokens.
fn unique_temp_path(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "monzoboiii_test_{name}_{}_{n}.toml",
        std::process::id()
    ))
}

#[tokio::test]
async fn reauth_redirects_to_monzo_with_expected_params() {
    let monzo = Arc::new(MonzoClient::new(
        Tokens::default(),
        PathBuf::from("/dev/null"),
        test_config(),
    ));
    let app = build_app(monzo);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/reauth")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = res
        .headers()
        .get("location")
        .expect("redirect must include a Location header")
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.starts_with("https://auth.monzo.com/"));
    assert!(location.contains("client_id=client_id"));
    assert!(location.contains("response_type=code"));
    assert!(location.contains("redirect_uri=http%3A%2F%2Flocalhost%2Fcallback"));
    assert!(location.contains("state="));
}

#[tokio::test]
async fn callback_exchanges_code_and_persists_tokens() {
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = Router::new().route(
        "/oauth2/token",
        post(|| async {
            Json(json!({
                "access_token": "new_access",
                "refresh_token": "new_refresh"
            }))
        }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let tokens_path = unique_temp_path("callback_ok");
    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), tokens_path.clone(), test_config())
            .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
    let app = build_app(monzo);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/callback?code=auth_code_123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let saved = std::fs::read_to_string(&tokens_path).expect("tokens.toml should have been written");
    assert!(saved.contains("new_access"));
    assert!(saved.contains("new_refresh"));
    let _ = std::fs::remove_file(&tokens_path);
}

#[tokio::test]
async fn callback_with_failed_exchange_returns_server_error() {
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    // Simulates Monzo rejecting the code (expired/already used/etc) — no
    // access_token in the body, matching Monzo's real error-response shape.
    let mock_app = Router::new().route(
        "/oauth2/token",
        post(|| async { Json(json!({"error": "invalid_grant"})) }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let tokens_path = unique_temp_path("callback_fail");
    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), tokens_path.clone(), test_config())
            .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
    let app = build_app(monzo);

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/callback?code=bad_code")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !tokens_path.exists(),
        "tokens must not be written on a failed exchange"
    );
}
