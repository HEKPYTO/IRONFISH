use serde_json::json;
use crate::helpers::{TestServer, TEST_ADMIN_KEY};

#[tokio::test]
async fn test_health_endpoint() {
    let server = TestServer::new().await;
    let resp = server.get("/v1/health").await;

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["status"], "healthy");
    assert!(body["node_id"].is_string());
}

#[tokio::test]
async fn test_analyze_endpoint_mock() {
    let server = TestServer::new().await;

    let body = json!({
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "depth": 10
    });

    let resp = server.post_json("/v1/analyze", &body).await;
    assert_eq!(resp.status(), 200);

    let result: serde_json::Value = resp.json().await.expect("json");
    assert!(result["id"].is_string());
    assert_eq!(result["fen"], "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    assert!(result["best_move"].is_object());
    assert!(result["evaluation"].is_object());
}

#[tokio::test]
async fn test_analyze_invalid_fen() {
    let server = TestServer::new().await;

    let body = json!({
        "fen": "invalid-fen",
        "depth": 10
    });

    let resp = server.post_json("/v1/analyze", &body).await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_bestmove_endpoint_mock() {
    let server = TestServer::new().await;

    let body = json!({
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    });

    let resp = server.post_json("/v1/bestmove", &body).await;
    assert_eq!(resp.status(), 200);

    let result: serde_json::Value = resp.json().await.expect("json");
    assert!(result["best_move"].is_object());
}

#[tokio::test]
async fn test_token_create_and_list() {
    let server = TestServer::new().await;

    let create_body = json!({ "name": "test-api-token" });
    let resp = server.post_json("/_admin/tokens", &create_body).await;
    assert_eq!(resp.status(), 200);

    let created: serde_json::Value = resp.json().await.expect("json");
    assert!(created["id"].is_string());
    assert!(created["token"].is_string());

    let list_resp = server.get("/_admin/tokens").await;
    assert_eq!(list_resp.status(), 200);

    let tokens: serde_json::Value = list_resp.json().await.expect("json");
    assert!(tokens.is_array());
}

#[tokio::test]
async fn test_cluster_status() {
    let server = TestServer::new().await;

    let resp = server.get("/_admin/cluster/status").await;
    assert_eq!(resp.status(), 200);

    let status: serde_json::Value = resp.json().await.expect("json");
    assert!(status["nodes"].is_array());
    assert!(status["healthy"].is_boolean());
}

#[tokio::test]
async fn test_graphql_query() {
    let server = TestServer::new().await;

    let body = json!({
        "query": "{ clusterStatus { healthy term } }"
    });

    let resp = server.post_json("/graphql", &body).await;
    assert_eq!(resp.status(), 200);

    let result: serde_json::Value = resp.json().await.expect("json");
    assert!(result["data"]["clusterStatus"].is_object());
}

#[tokio::test]
async fn test_graphql_bestmove() {
    let server = TestServer::new().await;

    let body = json!({
        "query": r#"{ bestMove(fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1") { bestMove { from to } } }"#
    });

    let resp = server.post_json("/graphql", &body).await;
    assert_eq!(resp.status(), 200);

    let result: serde_json::Value = resp.json().await.expect("json");
    assert!(result["data"]["bestMove"]["bestMove"].is_object());
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let server = TestServer::new().await;

    let resp = server.get("/metrics").await;
    assert!(resp.status() == 200 || resp.status() == 404);
}

#[tokio::test]
async fn test_admin_auth_required() {
    let server = TestServer::with_auth().await;

    let resp = reqwest::Client::new()
        .get(server.url("/_admin/tokens"))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_admin_auth_invalid_key() {
    let server = TestServer::with_auth().await;

    let resp = reqwest::Client::new()
        .get(server.url("/_admin/tokens"))
        .header("X-Admin-Key", "wrong-key")
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_admin_auth_valid_key() {
    let server = TestServer::with_auth().await;

    let resp = server.admin_get("/_admin/tokens").await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_admin_create_token_with_auth() {
    let server = TestServer::with_auth().await;

    let body = json!({ "name": "new-token" });
    let resp = server.admin_post_json("/_admin/tokens", &body).await;

    assert_eq!(resp.status(), 200);

    let created: serde_json::Value = resp.json().await.expect("json");
    assert!(created["id"].is_string());
    assert!(created["token"].is_string());
    let token_str = created["token"].as_str().unwrap();
    assert!(token_str.starts_with("iff_"));
}

#[tokio::test]
async fn test_api_auth_required() {
    let server = TestServer::with_auth().await;

    let body = json!({
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "depth": 10
    });

    let resp = reqwest::Client::new()
        .post(server.url("/v1/analyze"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_api_auth_with_token() {
    let server = TestServer::with_auth().await;

    let body = json!({
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "depth": 10
    });

    let resp = server.post_json("/v1/analyze", &body).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_health_no_auth_required() {
    let server = TestServer::with_auth().await;

    let resp = reqwest::Client::new()
        .get(server.url("/v1/health"))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 200);
}
