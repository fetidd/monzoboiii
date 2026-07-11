use axum::{
    Json, Router,
    body::Body,
    extract::{Form, Path},
    http::{Request, StatusCode},
    response::IntoResponse,
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
        )
        .route("/feed", post(|| async { StatusCode::OK }));

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

/// Builds a `TransactionData` JSON object populated with every field the
/// webhook handler's strict deserialization requires, defaulted to a
/// Mastercard transaction eligible for pot cover. Fields in `extra` are
/// merged on top, overriding any default with the same key.
fn transaction_data(extra: serde_json::Value) -> serde_json::Value {
    let mut data = json!({
        "id": "tx_001",
        "account_id": "acc_123",
        "amount": -500,
        "amount_is_pending": false,
        "atm_fees_detailed": null,
        "attachments": null,
        "can_add_to_tab": false,
        "can_be_excluded_from_breakdown": false,
        "can_be_made_subscription": false,
        "can_match_transactions_in_categorization": false,
        "can_split_the_bill": false,
        "categories": null,
        "category": "eating_out",
        "counterparty": {},
        "created": "2026-01-01T12:00:00.000Z",
        "currency": "GBP",
        "dedupe_id": "dedupe_001",
        "description": "Test transaction",
        "fees": {},
        "include_in_spending": true,
        "international": null,
        "is_load": false,
        "labels": null,
        "local_amount": -500,
        "local_currency": "GBP",
        "merchant": null,
        "merchant_feedback_uri": "",
        "metadata": {
            "eligible_for_pot_cover": "true",
            "ledger_committed_timestamp_earliest": "2026-01-01T12:00:00.000Z",
            "ledger_committed_timestamp_latest": "2026-01-01T12:00:00.000Z",
            "ledger_insertion_id": "entryset_001"
        },
        "notes": "",
        "originator": false,
        "parent_account_id": "acc_123",
        "scheme": "mastercard",
        "settled": "",
        "updated": "2026-01-01T12:00:00.000Z",
        "user_id": "user_001"
    });

    if let (Some(data_obj), Some(extra_obj)) = (data.as_object_mut(), extra.as_object()) {
        for (key, value) in extra_obj {
            data_obj.insert(key.clone(), value.clone());
        }
    }

    data
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
                "data": transaction_data(json!({}))
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
                "data": transaction_data(json!({"category": "groceries"}))
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
                "data": transaction_data(json!({"category": "groceries"}))
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
                "data": transaction_data(json!({"account_id": "acc_other", "category": "groceries"}))
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
                "data": transaction_data(json!({"category": "bad_category", "amount": -300}))
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
            )
            .route("/feed", post(|| async { StatusCode::OK }));
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
                    "data": transaction_data(json!({"id": format!("tx_{i:03}"), "category": "groceries", "amount": -100}))
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
            )
            .route("/feed", post(|| async { StatusCode::OK }));
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
                "data": transaction_data(json!({"category": "groceries"}))
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

#[tokio::test]
async fn non_mastercard_scheme_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    // "eating_out" maps to an active pot in the mock, so if the scheme gate
    // were broken this would trigger a real withdrawal call.
    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": transaction_data(json!({
                    "category": "eating_out",
                    "scheme": "uk_retail_pot"
                }))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn not_eligible_for_pot_cover_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": transaction_data(json!({
                    "category": "eating_out",
                    "metadata": {"eligible_for_pot_cover": "false"}
                }))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn eligible_for_pot_cover_missing_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": transaction_data(json!({
                    "category": "eating_out",
                    "metadata": {}
                }))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn positive_amount_refund_returns_ok_without_withdrawal() {
    let (mock_url, calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": transaction_data(json!({
                    "category": "eating_out",
                    "amount": 500,
                    "local_amount": 500
                }))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(calls.lock().await.is_empty());
}

#[tokio::test]
async fn deleted_pot_is_excluded_from_withdrawal() {
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = Router::new().route(
        "/pots",
        get(|| async {
            Json(json!({
                "pots": [{"id": "pot_999", "name": "groceries", "deleted": true}]
            }))
        }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let monzo = Arc::new(
        MonzoClient::new(Tokens::default(), PathBuf::from("/dev/null"), test_config())
            .with_base_url(format!("http://127.0.0.1:{mock_port}")),
    );
    monzo.refresh_pot_map().await.unwrap();
    let app = build_app(monzo);

    // No /pots/{id}/withdraw route exists on this mock, so if the deleted
    // pot leaked into the map, the withdrawal attempt would 404 and this
    // would come back as a 500 instead of 200.
    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": transaction_data(json!({"category": "groceries"}))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn malformed_json_body_returns_bad_request() {
    let (mock_url, _calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/monzo/test_secret")
                .header("content-type", "application/json")
                .body(Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn webhook_missing_required_field_returns_bad_request() {
    let (mock_url, _calls) = spawn_mock_monzo().await;
    let app = build_test_app(mock_url).await;

    let mut data = transaction_data(json!({}));
    data.as_object_mut().unwrap().remove("scheme");

    let res = app
        .oneshot(webhook_request(
            "test_secret",
            json!({
                "type": "transaction.created",
                "data": data
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pot_map_refresh_retries_after_401() {
    let pots_attempts = Arc::new(AtomicU32::new(0));
    let refresh_calls = Arc::new(AtomicU32::new(0));

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    {
        let pots_attempts = pots_attempts.clone();
        let refresh_calls = refresh_calls.clone();
        let mock_app = Router::new()
            .route(
                "/pots",
                get(move || {
                    let attempt = pots_attempts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        // First call simulates an expired access token; the
                        // retry (after refresh) should succeed.
                        if attempt == 0 {
                            StatusCode::UNAUTHORIZED.into_response()
                        } else {
                            Json(json!({
                                "pots": [{"id": "pot_123", "name": "groceries", "deleted": false}]
                            }))
                            .into_response()
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

    monzo
        .refresh_pot_map()
        .await
        .expect("pot map refresh should recover after refreshing the token");

    assert_eq!(
        pots_attempts.load(Ordering::SeqCst),
        2,
        "expected initial 401 + retry"
    );
    assert_eq!(
        refresh_calls.load(Ordering::SeqCst),
        1,
        "expected exactly one token refresh"
    );
}

#[tokio::test]
async fn withdraw_fails_with_non_auth_error_returns_server_error() {
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
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
            put(|| async {
                // Simulates a genuine Monzo-side rejection, e.g. insufficient pot balance.
                StatusCode::UNPROCESSABLE_ENTITY
            }),
        );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

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
                "data": transaction_data(json!({"category": "groceries"}))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn feed_post_failure_after_successful_withdrawal_still_returns_ok() {
    let withdraw_calls: Arc<Mutex<Vec<(String, u64)>>> = Arc::new(Mutex::new(Vec::new()));

    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    {
        let withdraw_calls = withdraw_calls.clone();
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
                        let withdraw_calls = withdraw_calls.clone();
                        async move {
                            withdraw_calls.lock().await.push((pot_id, form.amount));
                            StatusCode::OK
                        }
                    },
                ),
            );
        // Deliberately no /feed route: it 404s, so error_for_status() there
        // fails — but that failure must only be logged, not surfaced.
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
                "data": transaction_data(json!({"category": "groceries"}))
            }),
        ))
        .await
        .unwrap();

    assert_eq!(
        withdraw_calls.lock().await.as_slice(),
        [("pot_123".to_string(), 500u64)]
    );
    assert_eq!(res.status(), StatusCode::OK);
}
