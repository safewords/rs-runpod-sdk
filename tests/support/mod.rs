//! Shared by the integration tests. Each test file is its own crate and uses
//! only part of this.
#![allow(dead_code)]

pub mod spec;

use wiremock::MockServer;

/// A client pointed at a mock server, with fast retries.
pub fn client(server: &MockServer) -> runpod::Client {
    runpod::Client::builder()
        .api_key("test-key")
        .base_url(server.uri())
        .build()
        .unwrap()
}

/// A pod as Runpod's own spec example shows it.
pub fn pod_json() -> serde_json::Value {
    spec::Examples::load().first_response("/v2/pods/{id}", "get", "200")
}

/// A queued pod as Runpod's own spec example shows it.
pub fn queued_pod_json() -> serde_json::Value {
    spec::Examples::load().first_response("/v2/queued-pods/{id}", "get", "200")
}

pub fn problem(status: u16, detail: &str) -> serde_json::Value {
    serde_json::json!({ "title": "Problem", "status": status, "detail": detail })
}
