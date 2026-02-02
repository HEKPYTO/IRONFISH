use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};
use ironfish_core::{
    ApiToken, ClusterDiscovery, ConsensusProtocol, GossipMessage, GossipProtocol, Result,
    TokenStore,
};
use crate::consensus::HybridConsensus;
use crate::discovery::DiscoveryManager;
use crate::gossip::GossipService;
use crate::membership::MembershipManager;
use crate::network::{GossipEnvelope, NetworkService};
use crate::node::SharedNode;
pub struct ClusterConfig {
    pub discovery_interval: Duration,
    pub gossip_interval: Duration,
    pub health_check_interval: Duration,
    pub multicast_group: String,
    pub multicast_port: u16,
    pub static_peers: Vec<String>,
    pub auto_join: bool,
}
impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            discovery_interval: Duration::from_secs(10),
            gossip_interval: Duration::from_secs(5),
            health_check_interval: Duration::from_secs(30),
            multicast_group: "239.255.42.98".to_string(),
            multicast_port: 7878,
            static_peers: Vec::new(),
            auto_join: true,
        }
    }
}
pub struct ClusterService<T: TokenStore + Send + Sync + 'static> {
    config: ClusterConfig,
    local_node: SharedNode,
    network: Arc<NetworkService>,
    gossip: Arc<GossipService>,
    consensus: Arc<HybridConsensus>,
    discovery: Arc<DiscoveryManager>,
    membership: Arc<MembershipManager>,
    token_store: Arc<T>,
    shutdown_tx: broadcast::Sender<()>,
    running: Arc<RwLock<bool>>,
}
impl<T: TokenStore + Send + Sync + 'static> ClusterService<T> {
    pub fn new(
        config: ClusterConfig,
        local_node: SharedNode,
        membership: Arc<MembershipManager>,
        token_store: Arc<T>,
    ) -> Result<Self> {
        let node_info = local_node.info().clone();
        let network = Arc::new(NetworkService::new(node_info.clone()));
        let gossip = Arc::new(GossipService::new(local_node.id().clone()));
        let consensus = Arc::new(HybridConsensus::new(local_node.clone()));
        let mut discovery = DiscoveryManager::new();
        if !config.static_peers.is_empty() {
            discovery = discovery.with_static(config.static_peers.clone());
        }
        discovery = discovery.with_multicast(&config.multicast_group, config.multicast_port)?;
        let (shutdown_tx, _) = broadcast::channel(1);
        Ok(Self {
            config,
            local_node,
            network,
            gossip,
            consensus,
            discovery: Arc::new(discovery),
            membership,
            token_store,
            shutdown_tx,
            running: Arc::new(RwLock::new(false)),
        })
    }
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            if *running {
                return Ok(());
            }
            *running = true;
        }
        self.network.start().await?;
        self.gossip.start().await?;
        self.consensus.start().await?;
        let local_info = self.local_node.info();
        self.discovery.announce(local_info).await?;
        self.start_discovery_loop().await;
        self.start_gossip_receiver().await;
        self.start_gossip_sync_loop().await;
        self.start_announcement_loop().await;
        info!("cluster service started for node {}", self.local_node.id());
        Ok(())
    }
    pub async fn stop(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        self.network.stop().await;
        self.gossip.stop().await?;
        self.consensus.stop().await?;
        self.discovery.withdraw(self.local_node.id()).await?;
        let mut running = self.running.write().await;
        *running = false;
        info!("cluster service stopped");
        Ok(())
    }
    async fn start_discovery_loop(&self) {
        let discovery = self.discovery.clone();
        let network = self.network.clone();
        let membership = self.membership.clone();
        let local_node = self.local_node.clone();
        let interval = self.config.discovery_interval;
        let auto_join = self.config.auto_join;
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            timer.tick().await;
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        match discovery.discover().await {
                            Ok(peers) => {
                                for peer in peers {
                                    if peer.id == *local_node.id() {
                                        continue;
                                    }
                                    network.add_peer(peer.clone()).await;
                                    if auto_join && !membership.is_member(&peer.id).await {
                                        membership.add_member(peer.clone()).await;
                                        debug!("auto-joined peer {}", peer.id);
                                    }
                                }
                                let peer_count = network.peer_count().await;
                                if peer_count > 0 {
                                    debug!("discovered {} peers", peer_count);
                                }
                            }
                            Err(e) => {
                                debug!("discovery error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
    }
    async fn start_gossip_receiver(&self) {
        let network = self.network.clone();
        let gossip = self.gossip.clone();
        let token_store = self.token_store.clone();
        let local_id = self.local_node.id().clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe_envelope = async {
                        let mut rx = network.incoming_rx.write().await;
                        rx.recv().await
                    } => {
                        if let Some(envelope) = maybe_envelope {
                            if envelope.origin == local_id {
                                continue;
                            }
                            if let Err(e) = process_gossip_message(&envelope, &token_store, &gossip).await {
                                warn!("failed to process gossip: {}", e);
                            }
                            if envelope.hops < 3 {
                                let forward = GossipEnvelope {
                                    hops: envelope.hops + 1,
                                    ..envelope
                                };
                                let _ = network.broadcast(forward).await;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
    }
    async fn start_gossip_sync_loop(&self) {
        let network = self.network.clone();
        let gossip = self.gossip.clone();
        let token_store = self.token_store.clone();
        let interval = self.config.gossip_interval;
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        let peers = network.healthy_peers().await;
                        if peers.is_empty() {
                            continue;
                        }
                        let idx = rand::random_index(peers.len());
                        let peer = &peers[idx];
                        match network.sync_with_peer(&peer.id, 0).await {
                            Ok(entries) => {
                                for envelope in entries {
                                    if let Err(e) = process_gossip_message(&envelope, &token_store, &gossip).await {
                                        debug!("sync message error: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("sync with {} failed: {}", peer.id, e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
    }
    async fn start_announcement_loop(&self) {
        let discovery = self.discovery.clone();
        let local_node = self.local_node.clone();
        let interval = self.config.discovery_interval;
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        let info = local_node.info();
                        if let Err(e) = discovery.announce(info).await {
                            debug!("announcement failed: {}", e);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
    }
    pub async fn broadcast_token_created(&self, token: ApiToken) -> Result<()> {
        let envelope = GossipEnvelope {
            message: GossipMessage::TokenCreated(token),
            origin: self.local_node.id().clone(),
            version: chrono::Utc::now().timestamp_millis() as u64,
            hops: 0,
        };
        self.gossip.broadcast(envelope.message.clone()).await?;
        self.network.broadcast(envelope).await
    }
    pub async fn broadcast_token_revoked(&self, token_id: uuid::Uuid) -> Result<()> {
        let envelope = GossipEnvelope {
            message: GossipMessage::TokenRevoked(token_id),
            origin: self.local_node.id().clone(),
            version: chrono::Utc::now().timestamp_millis() as u64,
            hops: 0,
        };
        self.gossip.broadcast(envelope.message.clone()).await?;
        self.network.broadcast(envelope).await
    }
    pub fn gossip(&self) -> Arc<GossipService> {
        self.gossip.clone()
    }
    pub fn network(&self) -> Arc<NetworkService> {
        self.network.clone()
    }
    pub async fn peer_count(&self) -> usize {
        self.network.peer_count().await
    }
}
async fn process_gossip_message<T: TokenStore>(
    envelope: &GossipEnvelope,
    token_store: &Arc<T>,
    _gossip: &Arc<GossipService>,
) -> Result<()> {
    match &envelope.message {
        GossipMessage::TokenCreated(token) => match token_store.get(&token.id).await {
            Ok(Some(existing)) => {
                if token.created_at > existing.created_at {
                    token_store.update(token.clone()).await?;
                    info!("updated token {} from gossip", token.id);
                }
            }
            Ok(None) => {
                token_store.create(token.clone()).await?;
                info!("replicated token {} from gossip", token.id);
            }
            Err(e) => {
                warn!("failed to check token {}: {}", token.id, e);
            }
        },
        GossipMessage::TokenRevoked(token_id) => {
            if let Err(e) = token_store.revoke(token_id).await {
                debug!("revoke token {} error: {}", token_id, e);
            } else {
                info!("revoked token {} from gossip", token_id);
            }
        }
        GossipMessage::TokenUpdated(token) => {
            if let Err(e) = token_store.update(token.clone()).await {
                debug!("update token {} error: {}", token.id, e);
            }
        }
        GossipMessage::NodeJoined(node_info) => {
            debug!("node {} joined via gossip", node_info.id);
        }
        GossipMessage::NodeLeft(node_id) => {
            debug!("node {} left via gossip", node_id);
        }
        GossipMessage::NodeMetrics(node_id, _metrics) => {
            debug!("received metrics from {}", node_id);
        }
    }
    Ok(())
}
pub mod rand {
    use std::time::{SystemTime, UNIX_EPOCH};
    pub fn random_index(max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as usize;
        nanos % max
    }
}
