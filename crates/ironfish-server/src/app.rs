use std::sync::Arc;
use chrono::Utc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::info;

use ironfish_api::{ApiRouter, ApiState};
use ironfish_auth::SledTokenStore;
use ironfish_auth::TokenManager;
use ironfish_cluster::{ClusterConfig, ClusterService, GossipEnvelope, MembershipManager, Node, NodeConfig};
use ironfish_core::GossipMessage;
use ironfish_stockfish::{AnalysisService, EnginePool, EnginePoolConfig};
use crate::config::Config;

pub struct Application {
    config: Config,
    state: Arc<ApiState>,
    cluster: Option<Arc<ClusterService<SledTokenStore>>>,
    gossip_tx: broadcast::Sender<GossipMessage>,
}

impl Application {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let node_config = NodeConfig {
            id: if config.node.id == "auto" {
                None
            } else {
                Some(config.node.id.clone())
            },
            bind_address: config.node.bind_address,
            priority: config.node.priority,
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let node = Arc::new(Node::new(node_config));
        info!(node_id = %node.id(), "node initialized");

        let engine_config = EnginePoolConfig {
            binary_path: config.stockfish.binary_path.clone(),
            pool_size: config.stockfish.pool_size,
        };

        let pool = Arc::new(EnginePool::new(engine_config).await?);
        info!(pool_size = config.stockfish.pool_size, "engine pool created");

        let analysis = Arc::new(AnalysisService::new(pool));

        let data_dir = config.node.data_dir.join("tokens");
        std::fs::create_dir_all(&data_dir)?;

        let token_store = Arc::new(SledTokenStore::new(&data_dir)?);

        let secret = config.auth.token_secret.as_bytes();
        let token_manager = Arc::new(
            TokenManager::new(secret, node.id().to_string())
                .with_default_ttl(config.auth.token_ttl_days),
        );

        let membership = Arc::new(MembershipManager::new(node.clone()));

        let (gossip_tx, _) = broadcast::channel::<GossipMessage>(1024);

        let state = Arc::new(
            ApiState::new(
                analysis,
                token_store.clone(),
                token_manager,
                node.clone(),
                membership.clone(),
            )
            .with_gossip(gossip_tx.clone()),
        );

        let cluster = if config.cluster.enabled {
            let cluster_config = ClusterConfig {
                discovery_interval: std::time::Duration::from_millis(config.cluster.gossip_interval_ms),
                gossip_interval: std::time::Duration::from_millis(config.cluster.gossip_interval_ms),
                health_check_interval: std::time::Duration::from_millis(config.cluster.heartbeat_interval_ms),
                multicast_group: config.discovery.multicast_group.clone(),
                multicast_port: config.discovery.multicast_port,
                static_peers: config.discovery.static_peers.clone(),
                auto_join: true,
            };

            match ClusterService::new(cluster_config, node, membership, token_store) {
                Ok(service) => {
                    info!("cluster service initialized");
                    Some(Arc::new(service))
                }
                Err(e) => {
                    info!("cluster service disabled: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self { config, state, cluster, gossip_tx })
    }

    pub async fn run(self) -> anyhow::Result<()> {
        if let Some(ref cluster) = self.cluster {
            cluster.start().await?;
            info!("cluster service started");

            let cluster_clone = cluster.clone();
            let node_id = self.state.node.id().clone();
            let mut gossip_rx = self.gossip_tx.subscribe();

            tokio::spawn(async move {
                while let Ok(msg) = gossip_rx.recv().await {
                    let envelope = GossipEnvelope {
                        message: msg,
                        origin: node_id.clone(),
                        version: Utc::now().timestamp_millis() as u64,
                        hops: 0,
                    };
                    if let Err(e) = cluster_clone.network().broadcast(envelope).await {
                        tracing::debug!("gossip broadcast error: {}", e);
                    }
                }
            });
        }

        let rest_router = ApiRouter::new(self.state.clone())
            .with_auth(self.config.auth.enabled)
            .build_rest_router();

        let grpc_router = ApiRouter::new(self.state.clone())
            .build_grpc_router();

        let http_addr = self.config.node.bind_address;
        let grpc_port = self.config.node.bind_address.port() + 1;
        let grpc_addr = std::net::SocketAddr::new(
            self.config.node.bind_address.ip(),
            grpc_port,
        );

        let listener = TcpListener::bind(http_addr).await?;
        info!(address = %http_addr, "REST/GraphQL server listening");

        let grpc_handle = tokio::spawn(async move {
            info!(address = %grpc_addr, "gRPC server listening");
            grpc_router
                .serve(grpc_addr)
                .await
                .expect("gRPC server error");
        });

        let http_handle = tokio::spawn(async move {
            axum::serve(listener, rest_router)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .expect("HTTP server error");
        });

        tokio::select! {
            _ = http_handle => {},
            _ = grpc_handle => {},
        }

        if let Some(ref cluster) = self.cluster {
            let _ = cluster.stop().await;
        }

        info!("server shutdown complete");
        Ok(())
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("shutdown signal received");
}
