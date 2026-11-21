//! What the generated operations put on the wire and how they read answers.

mod support;

use runpod::{
    CatalogInclude, CreatePodRequest, CreatePodResponse, GpuCloudFilter, ListAuditLogsParams,
    ListGpuTypesParams, ListPodsParams, PodAction, PodActionRequest, ResourceFilters, ResourceType,
    TagCondition,
};
use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn query_parameters_are_encoded_as_documented() {
    let server = MockServer::start().await;
    // `filters` is JSON in the query string.
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(query_param("includeClusterPods", "true"))
        .and(query_param(
            "filters",
            r#"{"tags":[{"key":"env","values":["dev","prod"]}]}"#,
        ))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pods": [], "pagination": { "nextCursor": null, "hasNextPage": false }
        })))
        .expect(1)
        .mount(&server)
        .await;
    // Comma-separated arrays (`explode: false`), and enums by wire value.
    Mock::given(method("GET"))
        .and(path("/v2/catalog/gpus"))
        .and(query_param("include", "AVAILABILITY"))
        .and(query_param("cloud", "SECURE"))
        .and(query_param("countryCodes", "FR,DE"))
        .and(query_param_is_missing("count"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "gpus": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/audit-logs"))
        .and(query_param("actorIds", "u1,u2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "auditLogs": [], "pagination": { "nextCursor": null, "hasNextPage": false }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = support::client(&server);
    client
        .list_pods(&ListPodsParams {
            include_cluster_pods: Some(true),
            filters: Some(ResourceFilters {
                tags: Some(vec![TagCondition {
                    key: "env".into(),
                    values: Some(vec!["dev".into(), "prod".into()]),
                }]),
            }),
            limit: Some(5),
            ..Default::default()
        })
        .await
        .unwrap();
    client
        .list_gpu_types(&ListGpuTypesParams {
            include: Some(vec![CatalogInclude::Availability]),
            cloud: Some(GpuCloudFilter::Secure),
            country_codes: Some(vec!["FR".into(), "DE".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    client
        .list_audit_logs(&ListAuditLogsParams {
            actor_ids: Some(vec!["u1".into(), "u2".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn path_ids_are_escaped() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/tags/a%2Fb/resources/SERVERLESS_ENDPOINT/ep%201"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    support::client(&server)
        .attach_tag_resource("a/b", &ResourceType::ServerlessEndpoint, "ep 1")
        .await
        .unwrap();
}

#[tokio::test]
async fn the_all_variant_follows_the_cursor() {
    let server = MockServer::start().await;
    let pod = support::pod_json();
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pods": [pod], "pagination": { "nextCursor": "page2", "hasNextPage": true }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/pods"))
        .and(query_param("cursor", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pods": [pod, pod], "pagination": { "nextCursor": null, "hasNextPage": false }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let pods = support::client(&server)
        .list_pods_all(&Default::default())
        .await
        .unwrap();
    assert_eq!(pods.len(), 3);
}

#[tokio::test]
async fn create_pod_tells_a_pod_from_a_queued_pod() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/pods"))
        .respond_with(ResponseTemplate::new(201).set_body_json(support::pod_json()))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/pods"))
        .respond_with(ResponseTemplate::new(202).set_body_json(support::queued_pod_json()))
        .mount(&server)
        .await;
    let client = support::client(&server);
    let body = CreatePodRequest {
        name: "worker".into(),
        image: Some("runpod/base:latest".into()),
        ..Default::default()
    };
    assert!(matches!(
        client.create_pod(&body).await.unwrap(),
        CreatePodResponse::Created(_)
    ));
    assert!(matches!(
        client.create_pod(&body).await.unwrap(),
        CreatePodResponse::Accepted(_)
    ));

    let sent: serde_json::Value = server.received_requests().await.unwrap()[0]
        .body_json()
        .unwrap();
    assert_eq!(
        sent,
        json!({ "name": "worker", "image": "runpod/base:latest" }),
        "unset fields are omitted"
    );
}

#[tokio::test]
async fn no_content_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/pods/p1/action"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/v2/pods/p1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = support::client(&server);
    let answer = client
        .pod_action(
            "p1",
            &PodActionRequest {
                action: PodAction::Terminate,
            },
        )
        .await
        .unwrap();
    assert_eq!(answer, None);
    client.delete_pod("p1").await.unwrap();
}

#[tokio::test]
async fn markdown_answers_are_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/templates/t1/readme"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("# Hello\n", "text/markdown; charset=utf-8"),
        )
        .mount(&server)
        .await;
    let readme = support::client(&server)
        .get_template_readme("t1")
        .await
        .unwrap();
    assert_eq!(readme, "# Hello\n");
}

#[test]
fn an_unknown_enum_value_still_decodes() {
    let status: runpod::PodStatus = serde_json::from_value(json!("HIBERNATING")).unwrap();
    assert_eq!(status, runpod::PodStatus::Other("HIBERNATING".into()));
    assert_eq!(serde_json::to_value(&status).unwrap(), json!("HIBERNATING"));
    assert_eq!(runpod::PodStatus::Running.to_string(), "RUNNING");
}

#[test]
fn credentials_are_redacted_from_debug() {
    let secret = runpod::CreateSecretRequest {
        name: "hf".into(),
        value: "hf_supersecret".into(),
        description: None,
    };
    let shown = format!("{secret:?}");
    assert!(!shown.contains("hf_supersecret"), "{shown}");
    assert!(shown.contains("hf"));
}
