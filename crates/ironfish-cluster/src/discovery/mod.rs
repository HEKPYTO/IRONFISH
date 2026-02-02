mod dns;
mod multicast;
mod seed;
mod static_conf;
use async_trait::async_trait;
pub use dns::DnsDiscovery;
use ironfish_core::{ClusterDiscovery, NodeId, NodeInfo, Result};
pub use multicast::MulticastDiscovery;
pub use seed::SeedDiscovery;
pub use static_conf::StaticDiscovery;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
pub struct DiscoveryManager {
    static_discovery: Option<StaticDiscovery>,
    seed_discovery: Option<SeedDiscovery>,
    multicast_discovery: Option<MulticastDiscovery>,
    dns_discovery: Option<DnsDiscovery>,
    known_peers: Arc<RwLock<Vec<NodeInfo>>>,
}
impl DiscoveryManager {
    pub fn new() -> Self {
        Self {
            static_discovery: None,
            seed_discovery: None,
            multicast_discovery: None,
            dns_discovery: None,
            known_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }
    pub fn with_static(mut self, peers: Vec<String>) -> Self {
        if !peers.is_empty() {
            self.static_discovery = Some(StaticDiscovery::new(peers));
        }
        self
    }
    pub fn with_seeds(mut self, seeds: Vec<String>) -> Self {
        if !seeds.is_empty() {
            self.seed_discovery = Some(SeedDiscovery::new(seeds));
        }
        self
    }
    pub fn with_multicast(mut self, group: &str, port: u16) -> Result<Self> {
        self.multicast_discovery = Some(MulticastDiscovery::new(group, port)?);
        Ok(self)
    }
    pub fn with_dns(mut self, hostname: String, port: u16) -> Self {
        if !hostname.is_empty() {
            self.dns_discovery = Some(DnsDiscovery::new(hostname, port));
        }
        self
    }
    pub async fn start(&self, local_node: &NodeInfo) -> Result<()> {
        if let Some(ref multicast) = self.multicast_discovery {
            multicast.announce(local_node).await?;
        }
        Ok(())
    }
    pub async fn stop(&self, node_id: &NodeId) -> Result<()> {
        if let Some(ref multicast) = self.multicast_discovery {
            multicast.withdraw(node_id).await?;
        }
        Ok(())
    }
}
impl Default for DiscoveryManager {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl ClusterDiscovery for DiscoveryManager {
    async fn discover(&self) -> Result<Vec<NodeInfo>> {
        let mut all_peers = Vec::new();
        if let Some(ref static_disc) = self.static_discovery {
            match static_disc.discover().await {
                Ok(peers) => {
                    debug!("static discovery found {} peers", peers.len());
                    all_peers.extend(peers);
                }
                Err(e) => debug!("static discovery failed: {}", e),
            }
        }
        if let Some(ref seed_disc) = self.seed_discovery {
            match seed_disc.discover().await {
                Ok(peers) => {
                    debug!("seed discovery found {} peers", peers.len());
                    all_peers.extend(peers);
                }
                Err(e) => debug!("seed discovery failed: {}", e),
            }
        }
        if let Some(ref multicast_disc) = self.multicast_discovery {
            match multicast_disc.discover().await {
                Ok(peers) => {
                    debug!("multicast discovery found {} peers", peers.len());
                    all_peers.extend(peers);
                }
                Err(e) => debug!("multicast discovery failed: {}", e),
            }
        }
        if let Some(ref dns_disc) = self.dns_discovery {
            match dns_disc.discover().await {
                Ok(peers) => {
                    debug!("dns discovery found {} peers", peers.len());
                    all_peers.extend(peers);
                }
                Err(e) => debug!("dns discovery failed: {}", e),
            }
        }
        let mut known = self.known_peers.write().await;
        for peer in &all_peers {
            if !known.iter().any(|p| p.id == peer.id) {
                known.push(peer.clone());
            }
        }
        debug!("discovered {} total peers", known.len());
        Ok(known.clone())
    }
    async fn announce(&self, node: &NodeInfo) -> Result<()> {
        if let Some(ref multicast) = self.multicast_discovery {
            multicast.announce(node).await?;
        }
        Ok(())
    }
    async fn withdraw(&self, node_id: &NodeId) -> Result<()> {
        let mut known = self.known_peers.write().await;
        known.retain(|p| &p.id != node_id);
        if let Some(ref multicast) = self.multicast_discovery {
            multicast.withdraw(node_id).await?;
        }
        Ok(())
    }
}
