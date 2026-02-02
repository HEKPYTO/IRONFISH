use std::net::{SocketAddr, ToSocketAddrs};
use async_trait::async_trait;
use chrono::Utc;
use tracing::debug;
use ironfish_core::{ClusterDiscovery, NodeId, NodeInfo, Result};
pub struct StaticDiscovery {
    peers: Vec<String>,
}
impl StaticDiscovery {
    pub fn new(peers: Vec<String>) -> Self {
        Self { peers }
    }
    fn resolve_peer(peer: &str) -> Option<(String, SocketAddr)> {
        if let Ok(addr) = peer.parse::<SocketAddr>() {
            return Some((peer.to_string(), addr));
        }
        match peer.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    let name = peer.split(':').next().unwrap_or(peer);
                    return Some((name.to_string(), addr));
                }
            }
            Err(e) => {
                debug!("failed to resolve peer {}: {}", peer, e);
            }
        }
        None
    }
}
#[async_trait]
impl ClusterDiscovery for StaticDiscovery {
    async fn discover(&self) -> Result<Vec<NodeInfo>> {
        let mut nodes = Vec::new();
        for peer in &self.peers {
            if let Some((name, addr)) = Self::resolve_peer(peer) {
                let node = NodeInfo {
                    id: NodeId::from_string(&name),
                    address: addr,
                    priority: 100,
                    started_at: Utc::now(),
                    version: "unknown".to_string(),
                };
                nodes.push(node);
            }
        }
        Ok(nodes)
    }
    async fn announce(&self, _node: &NodeInfo) -> Result<()> {
        Ok(())
    }
    async fn withdraw(&self, _node_id: &NodeId) -> Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_static_discovery_empty() {
        let disc = StaticDiscovery::new(vec![]);
        let nodes = disc.discover().await.unwrap();
        assert!(nodes.is_empty());
    }
    #[tokio::test]
    async fn test_static_discovery_valid_peers() {
        let peers = vec![
            "192.168.1.10:8080".to_string(),
            "192.168.1.11:8080".to_string(),
        ];
        let disc = StaticDiscovery::new(peers);
        let nodes = disc.discover().await.unwrap();
        assert_eq!(nodes.len(), 2);
    }
    #[tokio::test]
    async fn test_static_discovery_invalid_peers() {
        let peers = vec!["invalid-address".to_string()];
        let disc = StaticDiscovery::new(peers);
        let nodes = disc.discover().await.unwrap();
        assert!(nodes.is_empty());
    }
    #[tokio::test]
    async fn test_static_discovery_mixed_peers() {
        let peers = vec![
            "192.168.1.10:8080".to_string(),
            "invalid".to_string(),
            "10.0.0.1:9000".to_string(),
        ];
        let disc = StaticDiscovery::new(peers);
        let nodes = disc.discover().await.unwrap();
        assert_eq!(nodes.len(), 2);
    }
}
