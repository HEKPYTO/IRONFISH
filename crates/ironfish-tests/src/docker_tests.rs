use crate::helpers::{DockerCluster, TEST_ADMIN_KEY};
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_single_node_docker() {
    let cluster = DockerCluster::start(1).await;
    assert!(cluster.wait_healthy(60).await, "cluster not healthy");
    let resp = reqwest::get(&format!("{}/v1/health", cluster.nodes[0]))
        .await
        .expect("health check");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["status"], "healthy");
    cluster.stop().await;
}
#[tokio::test]
#[serial]
async fn test_three_node_cluster() {
    let cluster = DockerCluster::start(3).await;
    assert!(cluster.wait_healthy(120).await, "cluster not healthy");
    for node_url in &cluster.nodes {
        let resp = reqwest::get(&format!("{}/v1/health", node_url))
            .await
            .expect("health check");
        assert_eq!(resp.status(), 200);
    }
    cluster.stop().await;
}
#[tokio::test]
#[serial]
async fn test_cluster_analysis_distribution() {
    let cluster = DockerCluster::start(3).await;
    assert!(cluster.wait_healthy(120).await, "cluster not healthy");
    let client = reqwest::Client::new();
    let token_resp = client
        .post(format!("{}/_admin/tokens", cluster.nodes[0]))
        .header("X-Admin-Key", TEST_ADMIN_KEY)
        .json(&json!({ "name": "test" }))
        .send()
        .await
        .expect("create token");
    if token_resp.status() != 200 {
        panic!("create token failed: {:?}", token_resp.text().await);
    }
    let token_data: serde_json::Value = token_resp.json().await.expect("json");
    let token = token_data["token"].as_str().expect("token string");
    let analysis_body = json!({
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "depth": 10
    });
    for node_url in &cluster.nodes {
        let resp = client
            .post(format!("{}/v1/analyze", node_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&analysis_body)
            .send()
            .await
            .expect("analyze");
        assert_eq!(resp.status(), 200);
    }
    cluster.stop().await;
}
#[tokio::test]
#[serial]
async fn test_cluster_token_replication() {
    let cluster = DockerCluster::start(3).await;
    assert!(cluster.wait_healthy(120).await, "cluster not healthy");
    let client = reqwest::Client::new();
    let token_resp = client
        .post(format!("{}/_admin/tokens", cluster.nodes[0]))
        .header("X-Admin-Key", TEST_ADMIN_KEY)
        .json(&json!({ "name": "replicated-token" }))
        .send()
        .await
        .expect("create token");
    assert_eq!(token_resp.status(), 200);
    let token_data: serde_json::Value = token_resp.json().await.expect("json");
    let token = token_data["token"].as_str().expect("token string");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    for node_url in &cluster.nodes {
        let resp = client
            .get(format!("{}/v1/health", node_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .expect("health with token");
        assert_eq!(resp.status(), 200);
    }
    cluster.stop().await;
}
#[tokio::test]
#[serial]
async fn test_cluster_node_failure_recovery() {
    let cluster = DockerCluster::start(3).await;
    assert!(cluster.wait_healthy(120).await, "cluster not healthy");
    std::process::Command::new("docker")
        .args(["stop", "ironfish-node2"])
        .output()
        .expect("stop node2");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    assert!(cluster.health_check(0).await);
    assert!(!cluster.health_check(1).await);
    assert!(cluster.health_check(2).await);
    std::process::Command::new("docker")
        .args(["start", "ironfish-node2"])
        .output()
        .expect("start node2");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    assert!(cluster.health_check(1).await);
    cluster.stop().await;
}
#[tokio::test]
#[serial]
async fn test_cluster_graphql_across_nodes() {
    let cluster = DockerCluster::start(3).await;
    assert!(cluster.wait_healthy(120).await, "cluster not healthy");

    let client = reqwest::Client::new();

    let token_resp = client
        .post(format!("{}/_admin/tokens", cluster.nodes[0]))
        .header("X-Admin-Key", TEST_ADMIN_KEY)
        .json(&json!({ "name": "graphql-test" }))
        .send()
        .await
        .expect("create token");

    if token_resp.status() != 200 {
        panic!("create token failed: {:?}", token_resp.text().await);
    }

    let token_data: serde_json::Value = token_resp.json().await.expect("json");
    let token = token_data["token"].as_str().expect("token string");

    let body = json!({
        "query": "{ clusterStatus { nodes { id state } healthy term } }"
    });

    for node_url in &cluster.nodes {
        let resp = client
            .post(format!("{}/graphql", node_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await
            .expect("graphql");

        assert_eq!(resp.status(), 200);

        let result: serde_json::Value = resp.json().await.expect("json");
        assert!(result["data"]["clusterStatus"]["healthy"]
            .as_bool()
            .unwrap_or(false));
    }

    cluster.stop().await;
}

#[tokio::test]
#[serial]
async fn test_multiplexed_grpc() {
    use ironfish_api::proto::chess_analysis_client::ChessAnalysisClient;
    use ironfish_api::proto::AnalyzeRequest;
    let cluster = DockerCluster::start(1).await;
    assert!(cluster.wait_healthy(60).await, "cluster not healthy");
    let addr = cluster.nodes[0].clone();
    let mut client = ChessAnalysisClient::connect(addr)
        .await
        .expect("failed to connect grpc");
    let request = tonic::Request::new(AnalyzeRequest {
        fen: "startpos".to_string(),
        depth: 10,
        multipv: 1,
        movetime_ms: None,
    });
    let response: Result<_, tonic::Status> = client.analyze(request).await;

    if let Err(status) = response {
        assert!(
            matches!(
                status.code(),
                tonic::Code::Unauthenticated | tonic::Code::Internal
            ),
            "Expected Unauthenticated or Internal, got: {:?}",
            status
        );
    }

    cluster.stop().await;
}
