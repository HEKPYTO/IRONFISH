use async_trait::async_trait;
use ironfish_core::{ClusterDiscovery, Error, NodeId, NodeInfo, Result};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, warn};
pub struct SeedDiscovery {
    seeds: Vec<String>,
    timeout: Duration,
}
impl SeedDiscovery {
    pub fn new(seeds: Vec<String>) -> Self {
        Self {
            seeds,
            timeout: Duration::from_secs(5),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    async fn query_seed(&self, seed: &str) -> Result<Vec<NodeInfo>> {
        let stream = timeout(self.timeout, TcpStream::connect(seed))
            .await
            .map_err(|_| Error::Discovery(format!("connection timeout to {}", seed)))?
            .map_err(|e| Error::Discovery(format!("connection failed to {}: {}", seed, e)))?;
        let _ = stream;
        debug!("connected to seed {}", seed);
        Ok(Vec::new())
    }
}
#[async_trait]
impl ClusterDiscovery for SeedDiscovery {
    async fn discover(&self) -> Result<Vec<NodeInfo>> {
        let mut all_nodes = Vec::new();
        for seed in &self.seeds {
            match self.query_seed(seed).await {
                Ok(nodes) => {
                    debug!("seed {} returned {} nodes", seed, nodes.len());
                    all_nodes.extend(nodes);
                }
                Err(e) => {
                    warn!("failed to query seed {}: {}", seed, e);
                }
            }
        }
        Ok(all_nodes)
    }
    async fn announce(&self, _node: &NodeInfo) -> Result<()> {
        Ok(())
    }
    async fn withdraw(&self, _node_id: &NodeId) -> Result<()> {
        Ok(())
    }
}
