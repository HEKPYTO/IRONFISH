use crate::node::SharedNode;
use ironfish_core::{NodeId, NodeInfo, NodeState, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, info};
pub struct BullyElection {
    node: SharedNode,
    peers: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
    election_timeout: Duration,
}
impl BullyElection {
    pub fn new(node: SharedNode) -> Self {
        Self {
            node,
            peers: Arc::new(RwLock::new(HashMap::new())),
            election_timeout: Duration::from_secs(5),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.election_timeout = timeout;
        self
    }
    pub async fn add_peer(&self, peer: NodeInfo) {
        let mut peers = self.peers.write().await;
        peers.insert(peer.id.clone(), peer);
    }
    pub async fn remove_peer(&self, peer_id: &NodeId) {
        let mut peers = self.peers.write().await;
        peers.remove(peer_id);
    }
    pub async fn start_election(&self) -> Result<bool> {
        info!("starting bully election");
        self.node.set_state(NodeState::Candidate);
        let my_priority = self.node.priority();
        let peers = self.peers.read().await.clone();
        let higher_priority_peers: Vec<_> = peers
            .iter()
            .filter(|(_, info)| info.priority > my_priority)
            .collect();
        if higher_priority_peers.is_empty() {
            self.declare_victory().await;
            return Ok(true);
        }
        for (peer_id, _) in &higher_priority_peers {
            debug!("sending election message to {}", peer_id);
        }
        match timeout(self.election_timeout, self.wait_for_coordinator()).await {
            Ok(_) => {
                debug!("received coordinator message");
                self.node.set_state(NodeState::Follower);
                Ok(false)
            }
            Err(_) => {
                info!("election timeout, declaring victory");
                self.declare_victory().await;
                Ok(true)
            }
        }
    }
    async fn wait_for_coordinator(&self) {
        tokio::time::sleep(self.election_timeout).await;
    }
    async fn declare_victory(&self) {
        info!(
            "node {} is now leader (priority: {})",
            self.node.id(),
            self.node.priority()
        );
        self.node.set_state(NodeState::Leader);
        self.node.set_leader(Some(self.node.id().clone()));
        self.node.increment_term();
        let peers = self.peers.read().await.clone();
        for (peer_id, _) in peers {
            debug!("sending coordinator message to {}", peer_id);
        }
    }
    pub async fn handle_election_message(&self, from: &NodeId, from_priority: u32) {
        let my_priority = self.node.priority();
        if my_priority > from_priority {
            debug!(
                "responding to election from {} - I have higher priority",
                from
            );
            self.start_election().await.ok();
        }
    }
    pub async fn handle_coordinator_message(&self, leader_id: NodeId) {
        debug!("accepting {} as leader", leader_id);
        self.node.set_state(NodeState::Follower);
        self.node.set_leader(Some(leader_id));
    }
}
