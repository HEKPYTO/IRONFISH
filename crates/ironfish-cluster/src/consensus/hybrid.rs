use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{debug, info, warn};
use ironfish_core::{
    ConsensusProtocol, HeartbeatRequest, HeartbeatResponse, NodeId, NodeInfo, NodeState, Result,
    VoteRequest, VoteResponse,
};
use crate::node::SharedNode;
use super::bully::BullyElection;
use super::raft::RaftConsensus;
pub struct HybridConsensus {
    node: SharedNode,
    raft: Arc<RaftConsensus>,
    bully: Arc<BullyElection>,
    heartbeat_timeout: Duration,
    shutdown_tx: broadcast::Sender<()>,
}
#[allow(dead_code)]
impl HybridConsensus {
    pub fn new(node: SharedNode) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            node: node.clone(),
            raft: Arc::new(RaftConsensus::new(node.clone())),
            bully: Arc::new(BullyElection::new(node)),
            heartbeat_timeout: Duration::from_secs(5),
            shutdown_tx,
        }
    }
    pub fn with_heartbeat_timeout(mut self, timeout: Duration) -> Self {
        self.heartbeat_timeout = timeout;
        self
    }
    pub async fn add_peer(&self, peer: NodeInfo) {
        self.raft.add_peer(peer.clone()).await;
        self.bully.add_peer(peer).await;
    }
    pub async fn remove_peer(&self, peer_id: &NodeId) {
        self.raft.remove_peer(peer_id).await;
        self.bully.remove_peer(peer_id).await;
    }
    async fn run_leader_detection(&self) {
        let mut timer = interval(self.heartbeat_timeout);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut missed_heartbeats = 0;
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if matches!(self.node.state(), NodeState::Follower) {
                        missed_heartbeats += 1;
                        if missed_heartbeats >= 3 {
                            warn!("leader heartbeat timeout, starting election");
                            self.trigger_election().await;
                            missed_heartbeats = 0;
                        }
                    } else {
                        missed_heartbeats = 0;
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    }
    async fn trigger_election(&self) {
        info!("triggering hybrid election");
        match self.bully.start_election().await {
            Ok(won) => {
                if won {
                    info!("won bully election, becoming leader");
                } else {
                    debug!("lost bully election");
                }
            }
            Err(e) => {
                warn!("bully election failed: {}", e);
            }
        }
    }
}
#[async_trait]
impl ConsensusProtocol for HybridConsensus {
    async fn start(&self) -> Result<()> {
        self.raft.start().await?;
        self.node.set_state(NodeState::Follower);
        let node = self.node.clone();
        let heartbeat_timeout = self.heartbeat_timeout;
        let bully = self.bully.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut timer = interval(heartbeat_timeout);
            let mut missed = 0u32;
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        let state = node.state();
                        match state {
                            NodeState::Follower => {
                                missed += 1;
                                if missed >= 3 {
                                    warn!("heartbeat timeout, starting election");
                                    let _ = bully.start_election().await;
                                    missed = 0;
                                }
                            }
                            NodeState::Leader => {
                                missed = 0;
                            }
                            _ => {}
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
        info!("hybrid consensus started");
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        self.raft.stop().await?;
        self.node.set_state(NodeState::Leaving);
        info!("hybrid consensus stopped");
        Ok(())
    }
    async fn request_vote(&self, request: VoteRequest) -> Result<VoteResponse> {
        if request.priority > self.node.priority() {
            self.bully
                .handle_election_message(&request.candidate_id, request.priority)
                .await;
        }
        self.raft.request_vote(request).await
    }
    async fn append_entries(&self, request: HeartbeatRequest) -> Result<HeartbeatResponse> {
        self.raft.append_entries(request).await
    }
    fn state(&self) -> NodeState {
        self.raft.state()
    }
    fn term(&self) -> u64 {
        self.raft.term()
    }
    fn leader(&self) -> Option<NodeId> {
        self.raft.leader()
    }
}
