use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, info};

use ironfish_core::{Error, GossipMessage, GossipProtocol, NodeId, NodeInfo, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GossipEntry {
    message: GossipMessage,
    timestamp: DateTime<Utc>,
    origin: NodeId,
    version: u64,
}

pub struct GossipService {
    node_id: NodeId,
    entries: Arc<RwLock<HashMap<String, GossipEntry>>>,
    peers: Arc<RwLock<Vec<NodeInfo>>>,
    message_rx: Arc<RwLock<mpsc::Receiver<GossipMessage>>>,
    broadcast_tx: broadcast::Sender<GossipMessage>,
    sync_interval: Duration,
    shutdown_tx: broadcast::Sender<()>,
}

impl GossipService {
    pub fn new(node_id: NodeId) -> Self {
        let (_message_tx, message_rx) = mpsc::channel(1024);
        let (broadcast_tx, _) = broadcast::channel(1024);
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            node_id,
            entries: Arc::new(RwLock::new(HashMap::new())),
            peers: Arc::new(RwLock::new(Vec::new())),
            message_rx: Arc::new(RwLock::new(message_rx)),
            broadcast_tx,
            sync_interval: Duration::from_secs(5),
            shutdown_tx,
        }
    }

    pub fn with_sync_interval(mut self, interval: Duration) -> Self {
        self.sync_interval = interval;
        self
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GossipMessage> {
        self.broadcast_tx.subscribe()
    }

    pub async fn add_peer(&self, peer: NodeInfo) {
        let mut peers = self.peers.write().await;
        if !peers.iter().any(|p| p.id == peer.id) {
            peers.push(peer);
        }
    }

    pub async fn remove_peer(&self, peer_id: &NodeId) {
        let mut peers = self.peers.write().await;
        peers.retain(|p| &p.id != peer_id);
    }

    pub async fn start(&self) -> Result<()> {
        let peers = self.peers.clone();
        let sync_interval = self.sync_interval;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut timer = interval(sync_interval);

            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        let peers_snapshot = peers.read().await.clone();
                        for peer in peers_snapshot {
                            debug!("syncing with peer {}", peer.id);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });

        info!("gossip service started");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        info!("gossip service stopped");
        Ok(())
    }

    fn entry_key(message: &GossipMessage) -> String {
        match message {
            GossipMessage::TokenCreated(t) => format!("token:{}", t.id),
            GossipMessage::TokenRevoked(id) => format!("token:{}", id),
            GossipMessage::TokenUpdated(t) => format!("token:{}", t.id),
            GossipMessage::NodeJoined(n) => format!("node:{}", n.id),
            GossipMessage::NodeLeft(id) => format!("node:{}", id),
            GossipMessage::NodeMetrics(id, _) => format!("metrics:{}", id),
        }
    }

    async fn apply_message(&self, entry: &GossipEntry) {
        let key = Self::entry_key(&entry.message);
        let mut entries = self.entries.write().await;

        match entries.get(&key) {
            Some(existing) if existing.version >= entry.version => {
                return;
            }
            _ => {}
        }

        entries.insert(key, entry.clone());
        let _ = self.broadcast_tx.send(entry.message.clone());

        debug!("applied gossip message from {}", entry.origin);
    }
}

#[async_trait]
impl GossipProtocol for GossipService {
    async fn broadcast(&self, message: GossipMessage) -> Result<()> {
        let entry = GossipEntry {
            message: message.clone(),
            timestamp: Utc::now(),
            origin: self.node_id.clone(),
            version: Utc::now().timestamp_millis() as u64,
        };

        self.apply_message(&entry).await;

        let peers = self.peers.read().await.clone();
        for peer in peers {
            debug!("broadcasting to peer {}", peer.id);
        }

        Ok(())
    }

    async fn receive(&self) -> Result<GossipMessage> {
        let mut rx = self.message_rx.write().await;
        rx.recv()
            .await
            .ok_or_else(|| Error::Gossip("channel closed".into()))
    }

    async fn sync(&self, peer: &NodeId) -> Result<()> {
        debug!("syncing with peer {}", peer);

        let entries = self.entries.read().await.clone();
        for (_key, _entry) in entries {
            debug!("would send entry to {}", peer);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gossip_service_creation() {
        let node_id = NodeId::from_string("test-node");
        let service = GossipService::new(node_id.clone());

        assert_eq!(service.node_id, node_id);
    }

    #[tokio::test]
    async fn test_gossip_add_peer() {
        let service = GossipService::new(NodeId::from_string("local"));
        let peer = NodeInfo {
            id: NodeId::from_string("peer-1"),
            address: "127.0.0.1:8080".parse().unwrap(),
            priority: 100,
            started_at: Utc::now(),
            version: "1.0.0".to_string(),
        };

        service.add_peer(peer.clone()).await;

        let peers = service.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].id, peer.id);
    }

    #[tokio::test]
    async fn test_gossip_remove_peer() {
        let service = GossipService::new(NodeId::from_string("local"));
        let peer_id = NodeId::from_string("peer-1");
        let peer = NodeInfo {
            id: peer_id.clone(),
            address: "127.0.0.1:8080".parse().unwrap(),
            priority: 100,
            started_at: Utc::now(),
            version: "1.0.0".to_string(),
        };

        service.add_peer(peer).await;
        assert_eq!(service.peers.read().await.len(), 1);

        service.remove_peer(&peer_id).await;
        assert_eq!(service.peers.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_gossip_no_duplicate_peers() {
        let service = GossipService::new(NodeId::from_string("local"));
        let peer = NodeInfo {
            id: NodeId::from_string("peer-1"),
            address: "127.0.0.1:8080".parse().unwrap(),
            priority: 100,
            started_at: Utc::now(),
            version: "1.0.0".to_string(),
        };

        service.add_peer(peer.clone()).await;
        service.add_peer(peer.clone()).await;

        let peers = service.peers.read().await;
        assert_eq!(peers.len(), 1);
    }

    #[tokio::test]
    async fn test_gossip_subscribe() {
        let service = GossipService::new(NodeId::from_string("local"));
        let _rx = service.subscribe();
    }

    #[test]
    fn test_entry_key() {
        let token_msg = GossipMessage::TokenRevoked(uuid::Uuid::new_v4());
        let key = GossipService::entry_key(&token_msg);
        assert!(key.starts_with("token:"));

        let node_id = NodeId::from_string("test");
        let node_msg = GossipMessage::NodeLeft(node_id);
        let key = GossipService::entry_key(&node_msg);
        assert!(key.starts_with("node:"));
    }
}
