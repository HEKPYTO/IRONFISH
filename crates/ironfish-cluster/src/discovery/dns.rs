use async_trait::async_trait;
use chrono::Utc;
use ironfish_core::{ClusterDiscovery, NodeId, NodeInfo, Result};
use tokio::net::lookup_host;
use tracing::{debug, warn};
pub struct DnsDiscovery {
    hostname: String,
    port: u16,
}
impl DnsDiscovery {
    pub fn new(hostname: String, port: u16) -> Self {
        Self { hostname, port }
    }
}
#[async_trait]
impl ClusterDiscovery for DnsDiscovery {
    async fn discover(&self) -> Result<Vec<NodeInfo>> {
        let target = format!("{}:{}", self.hostname, self.port);
        let mut nodes = Vec::new();
        match lookup_host(&target).await {
            Ok(addrs) => {
                for addr in addrs {
                    let id_str = addr.to_string();
                    let node = NodeInfo {
                        id: NodeId::from_string(&id_str),
                        address: addr,
                        priority: 100,
                        started_at: Utc::now(),
                        version: "unknown".to_string(),
                    };
                    nodes.push(node);
                }
            }
            Err(e) => {
                warn!("dns lookup failed for {}: {}", target, e);
            }
        }
        debug!("dns discovery found {} peers for {}", nodes.len(), target);
        Ok(nodes)
    }
    async fn announce(&self, _node: &NodeInfo) -> Result<()> {
        Ok(())
    }
    async fn withdraw(&self, _node_id: &NodeId) -> Result<()> {
        Ok(())
    }
}
