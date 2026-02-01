use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

use ironfish_api::{ApiRouter, ApiState};
use ironfish_auth::{SledTokenStore, TokenManager};
use ironfish_cluster::{MembershipManager, Node, NodeConfig};
use ironfish_core::TokenStore;
use ironfish_stockfish::{AnalysisService, EnginePool, EnginePoolConfig};

pub const TEST_ADMIN_KEY: &str = "test-admin-secret-key-12345";

pub struct TestServer {
    pub addr: SocketAddr,
    pub token: String,
    pub admin_key: String,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub async fn new() -> Self {
        Self::with_config(false, false).await
    }

    pub async fn with_stockfish(enable_stockfish: bool) -> Self {
        Self::with_config(enable_stockfish, false).await
    }

    pub async fn with_auth() -> Self {
        Self::with_config(false, true).await
    }

    pub async fn with_config(enable_stockfish: bool, enable_auth: bool) -> Self {
        if enable_auth {
            std::env::set_var("IRONFISH_ADMIN_KEY", TEST_ADMIN_KEY);
        }

        let node_config = NodeConfig {
            id: Some(format!("test-node-{}", uuid::Uuid::new_v4())),
            bind_address: "127.0.0.1:0".parse().unwrap(),
            priority: 100,
            version: "test".to_string(),
        };

        let node = Arc::new(Node::new(node_config));

        let analysis = if enable_stockfish {
            let engine_config = EnginePoolConfig {
                binary_path: std::env::var("STOCKFISH_PATH")
                    .unwrap_or_else(|_| "/usr/local/bin/stockfish".to_string()),
                pool_size: 1,
            };
            let pool = Arc::new(EnginePool::new(engine_config).await.expect("engine pool"));
            Arc::new(AnalysisService::new(pool))
        } else {
            Arc::new(AnalysisService::new_mock())
        };

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let token_store = Arc::new(
            SledTokenStore::new(temp_dir.path()).expect("token store"),
        );

        let secret = TokenManager::generate_secret();
        let token_manager = Arc::new(TokenManager::new(&secret, "test"));

        let membership = Arc::new(MembershipManager::new(node.clone()));

        let state = Arc::new(ApiState::new(
            analysis,
            token_store.clone(),
            token_manager.clone(),
            node,
            membership,
        ));

        let router = ApiRouter::new(state.clone())
            .with_auth(enable_auth)
            .build_rest_router();

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });

        let (api_token, response) = token_manager
            .create(ironfish_core::CreateTokenRequest {
                name: Some("test-token".into()),
                expires_in_days: None,
                rate_limit: None,
            })
            .expect("create token");

        let _ = token_store.create(api_token).await;

        TestServer {
            addr,
            token: response.token,
            admin_key: TEST_ADMIN_KEY.to_string(),
            _handle: handle,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(self.url(path))
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .expect("request")
    }

    pub async fn post_json<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.url(path))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .expect("request")
    }

    pub async fn admin_get(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(self.url(path))
            .header("X-Admin-Key", &self.admin_key)
            .send()
            .await
            .expect("request")
    }

    pub async fn admin_post_json<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.url(path))
            .header("X-Admin-Key", &self.admin_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .expect("request")
    }

    pub async fn admin_delete(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .delete(self.url(path))
            .header("X-Admin-Key", &self.admin_key)
            .send()
            .await
            .expect("request")
    }
}

pub struct DockerCluster {
    pub nodes: Vec<String>,
}

impl DockerCluster {
    pub async fn start(node_count: usize) -> Self {
        let mut nodes = Vec::new();
        for i in 0..node_count {
            let port = 8080 + (i * 2);
            nodes.push(format!("http://localhost:{}", port));
        }

        std::process::Command::new("docker-compose")
            .args(["up", "-d", "--scale", &format!("node={}", node_count)])
            .current_dir(env!("CARGO_MANIFEST_DIR").replace("/crates/ironfish-tests", ""))
            .output()
            .expect("docker-compose up");

        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        Self { nodes }
    }

    pub async fn stop(&self) {
        std::process::Command::new("docker-compose")
            .args(["down"])
            .current_dir(env!("CARGO_MANIFEST_DIR").replace("/crates/ironfish-tests", ""))
            .output()
            .expect("docker-compose down");
    }

    pub async fn health_check(&self, node_idx: usize) -> bool {
        if node_idx >= self.nodes.len() {
            return false;
        }

        let url = format!("{}/v1/health", self.nodes[node_idx]);
        reqwest::get(&url)
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn wait_healthy(&self, timeout_secs: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < timeout_secs {
            let mut all_healthy = true;
            for i in 0..self.nodes.len() {
                if !self.health_check(i).await {
                    all_healthy = false;
                    break;
                }
            }
            if all_healthy {
                return true;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        false
    }
}
