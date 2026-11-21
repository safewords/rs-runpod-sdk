//! The Server-Sent Events log streams, against a mock server.

mod support;

use futures_util::StreamExt;
use runpod::{LogSource, LogStreamParams};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BODY: &str = "\
: connected\n\n\
id: 2026-06-01T12:02:03Z/000000000001\n\
data: {\"ts\":\"2026-06-01T12:02:03Z\",\"source\":\"container\",\"line\":\"Model loaded.\"}\n\n\
id: 2026-06-01T12:02:04Z/000000000002\n\
data: {\"ts\":\"2026-06-01T12:02:04Z\",\"source\":\"system\",\"line\":\"healthy\"}\n\n";

#[tokio::test]
async fn pod_logs_stream_and_resume() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods/p1/logs"))
        .and(query_param("tail", "10"))
        .and(query_param("source", "container"))
        .and(header("accept", "text/event-stream"))
        .and(header("last-event-id", "earlier"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(BODY, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let params = LogStreamParams {
        source: Some(LogSource::Container),
        tail: Some(10),
        last_event_id: Some("earlier".into()),
        ..Default::default()
    };
    let events: Vec<_> = support::client(&server)
        .stream_pod_logs("p1", &params)
        .await
        .unwrap()
        .collect()
        .await;
    let events: Vec<_> = events.into_iter().map(Result::unwrap).collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].line, "Model loaded.");
    assert_eq!(
        events[1].id.as_deref(),
        Some("2026-06-01T12:02:04Z/000000000002")
    );
    assert_eq!(events[1].source, LogSource::System);
}

#[tokio::test]
async fn worker_logs_use_both_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/serverless/ep1/workers/w1/logs"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(BODY, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let mut stream = support::client(&server)
        .stream_worker_logs("ep1", "w1", &Default::default())
        .await
        .unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap().line, "Model loaded.");
}

#[tokio::test]
async fn an_error_status_fails_the_call_not_the_stream() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/pods/gone/logs"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(support::problem(404, "pod not found")),
        )
        .mount(&server)
        .await;
    let err = support::client(&server)
        .stream_pod_logs("gone", &Default::default())
        .await
        .unwrap_err();
    assert!(err.is_not_found());
}
