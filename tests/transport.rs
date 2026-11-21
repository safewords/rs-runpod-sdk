//! Auth, retries, rate limits and error answers, against a mock server.

mod support;

use std::time::Duration;

use runpod::{Client, CreateAuthenticationAttemptRequest, CreateSecretRequest, Error};
use serde_json::json;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn empty_pods() -> serde_json::Value {
    json!({ "pods": [], "pagination": { "nextCursor": null, "hasNextPage": false } })
}

#[tokio::test]
async fn sends_the_key_as_a_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_pods()))
        .expect(1)
        .mount(&server)
        .await;
    let pods = support::client(&server)
        .list_pods(&Default::default())
        .await
        .unwrap();
    assert!(pods.pods.is_empty());
}

#[tokio::test]
async fn sends_the_team_header_when_asked() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(header("x-team-id", "team_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_pods()))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key("k")
        .team_id("team_1")
        .base_url(server.uri())
        .build()
        .unwrap();
    client.list_pods(&Default::default()).await.unwrap();
}

#[tokio::test]
async fn a_read_is_retried_after_a_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_pods()))
        .expect(1)
        .mount(&server)
        .await;
    support::client(&server)
        .list_pods(&Default::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_create_is_not_retried_after_a_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/account/secrets"))
        .respond_with(ResponseTemplate::new(503).set_body_json(support::problem(503, "busy")))
        .expect(1)
        .mount(&server)
        .await;
    let body = CreateSecretRequest {
        name: "s".into(),
        value: "v".into(),
        description: None,
    };
    let err = support::client(&server)
        .create_secret(&body)
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(503));
    assert!(err.is_transient());
}

#[tokio::test]
async fn a_create_is_retried_after_a_429_honouring_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/account/secrets"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(support::problem(
                    429,
                    "rate limit exceeded for the minute window",
                )),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/account/secrets"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "sec_1", "name": "s", "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let body = CreateSecretRequest {
        name: "s".into(),
        value: "v".into(),
        description: None,
    };
    let secret = support::client(&server).create_secret(&body).await.unwrap();
    assert_eq!(secret.id, "sec_1");
}

#[tokio::test]
async fn a_problem_answer_becomes_an_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods/nope"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "title": "Not Found", "status": 404, "detail": "pod not found", "errors": ["id: unknown"]
        })))
        .mount(&server)
        .await;
    let err = support::client(&server).get_pod("nope").await.unwrap_err();
    assert!(err.is_not_found());
    let api = err.api().unwrap();
    assert_eq!(api.detail.as_deref(), Some("pod not found"));
    assert_eq!(api.errors, vec!["id: unknown".to_string()]);
    assert!(err.to_string().contains("pod not found (id: unknown)"));
}

#[tokio::test]
async fn retries_give_up_after_max_attempts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2)
        .mount(&server)
        .await;
    let client = Client::builder()
        .api_key("k")
        .base_url(server.uri())
        .max_attempts(2)
        .build()
        .unwrap();
    let err = client.list_pods(&Default::default()).await.unwrap_err();
    assert_eq!(err.status(), Some(500));
}

#[tokio::test]
async fn the_rate_limit_headers_are_kept() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(empty_pods())
                .insert_header("RateLimit", r#""minute";r=41;t=12, "day";r=900;t=3600"#)
                .insert_header(
                    "RateLimit-Policy",
                    r#""minute";q=60;w=60, "day";q=1000;w=86400"#,
                ),
        )
        .mount(&server)
        .await;
    let client = support::client(&server);
    assert!(client.rate_limit().is_none());
    client.list_pods(&Default::default()).await.unwrap();
    let rl = client.rate_limit().unwrap();
    assert_eq!(rl.windows[0].remaining, 41);
    assert_eq!(rl.policies[1].window, Duration::from_secs(86400));
    assert_eq!(rl.wait(), None);
}

#[tokio::test]
async fn signup_works_without_a_key_and_sends_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/signup/authentication-attempts"))
        .and(header("idempotency-key", "attempt-1"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "attemptId": "att_1", "expiresAt": "2026-01-01T00:10:00Z", "resendAfter": "2026-01-01T00:01:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder().base_url(server.uri()).build().unwrap();
    let body = CreateAuthenticationAttemptRequest {
        email: "someone@example.com".into(),
        key_name: "laptop".into(),
        code_challenge: "x".repeat(43),
    };
    let attempt = client
        .create_authentication_attempt("attempt-1", &body)
        .await
        .unwrap();
    assert_eq!(attempt.attempt_id, "att_1");
    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.get("authorization").is_none());

    // ...but anything else needs one.
    let err = client.list_pods(&Default::default()).await.unwrap_err();
    assert!(matches!(err, Error::MissingApiKey));
}

#[tokio::test]
async fn the_openapi_document_needs_no_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/openapi.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "openapi": "3.1.0" })))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder().base_url(server.uri()).build().unwrap();
    let doc = client.openapi_document().await.unwrap();
    assert_eq!(doc["openapi"], "3.1.0");
    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn a_spent_quota_window_is_waited_out_before_the_next_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(header_exists("authorization"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(empty_pods())
                .insert_header("RateLimit", r#""minute";r=0;t=1"#),
        )
        .mount(&server)
        .await;
    let client = support::client(&server);
    client.list_pods(&Default::default()).await.unwrap();
    let started = std::time::Instant::now();
    client.list_pods(&Default::default()).await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "did not wait: {:?}",
        started.elapsed()
    );
}
