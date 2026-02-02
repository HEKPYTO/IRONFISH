use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;
use tracing::{debug, info};
use ironfish_core::{
    ConsensusProtocol, HeartbeatRequest, HeartbeatResponse, NodeId, NodeInfo, NodeState, Result,
    VoteRequest, VoteResponse,
};
use crate::node::SharedNode;
pub struct RaftConsensus {
    node: SharedNode,
    peers: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
    voted_for: Arc<RwLock<Option<NodeId>>>,
    commit_index: Arc<RwLock<u64>>,
    heartbeat_interval: Duration,
    election_timeout: Duration,
    shutdown_tx: broadcast::Sender<()>,
}
#[allow(dead_code)]
impl RaftConsensus {
    pub fn new(node: SharedNode) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            node,
            peers: Arc::new(RwLock::new(HashMap::new())),
            voted_for: Arc::new(RwLock::new(None)),
            commit_index: Arc::new(RwLock::new(0)),
            heartbeat_interval: Duration::from_millis(1000),
            election_timeout: Duration::from_millis(5000),
            shutdown_tx,
        }
    }
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }
    pub fn with_election_timeout(mut self, timeout: Duration) -> Self {
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
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
    async fn start_leader_loop(&self) {
        let mut heartbeat_timer = interval(self.heartbeat_interval);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        loop {
            tokio::select! {
                _ = heartbeat_timer.tick() => {
                    if !matches!(self.node.state(), NodeState::Leader) {
                        break;
                    }
                    self.send_heartbeats().await;
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    }
    async fn send_heartbeats(&self) {
        let peers = self.peers.read().await.clone();
        let commit_index = *self.commit_index.read().await;
        for (peer_id, _peer_info) in peers {
            let request = HeartbeatRequest {
                leader_id: self.node.id().clone(),
                term: self.node.term(),
                commit_index,
            };
            debug!("sending heartbeat to {}", peer_id);
            let _ = request;
        }
    }
    async fn start_election(&self) -> Result<bool> {
        let new_term = self.node.increment_term();
        self.node.set_state(NodeState::Candidate);
        info!("starting election for term {}", new_term);
        *self.voted_for.write().await = Some(self.node.id().clone());
        let peers = self.peers.read().await.clone();
        let mut votes_received = 1;
        let votes_needed = (peers.len() + 1).div_ceil(2);
        for peer_id in peers.keys() {
            let request = VoteRequest {
                candidate_id: self.node.id().clone(),
                term: new_term,
                priority: self.node.priority(),
            };
            debug!("requesting vote from {}", peer_id);
            let _ = request;
            votes_received += 1;
        }
        if votes_received >= votes_needed {
            self.become_leader().await;
            Ok(true)
        } else {
            self.node.set_state(NodeState::Follower);
            Ok(false)
        }
    }
    async fn become_leader(&self) {
        info!("became leader for term {}", self.node.term());
        self.node.set_state(NodeState::Leader);
        self.node.set_leader(Some(self.node.id().clone()));
        let node = self.node.clone();
        let heartbeat_interval = self.heartbeat_interval;
        tokio::spawn(async move {
            let mut timer = interval(heartbeat_interval);
            loop {
                timer.tick().await;
                if !matches!(node.state(), NodeState::Leader) {
                    break;
                }
            }
        });
    }
}
#[async_trait]
impl ConsensusProtocol for RaftConsensus {
    async fn start(&self) -> Result<()> {
        self.node.set_state(NodeState::Follower);
        info!("raft consensus started");
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        self.node.set_state(NodeState::Leaving);
        info!("raft consensus stopped");
        Ok(())
    }
    async fn request_vote(&self, request: VoteRequest) -> Result<VoteResponse> {
        let current_term = self.node.term();
        if request.term < current_term {
            return Ok(VoteResponse {
                node_id: self.node.id().clone(),
                term: current_term,
                vote_granted: false,
            });
        }
        if request.term > current_term {
            self.node.set_term(request.term);
            self.node.set_state(NodeState::Follower);
            *self.voted_for.write().await = None;
        }
        let voted_for = self.voted_for.read().await.clone();
        let vote_granted = match voted_for {
            None => {
                *self.voted_for.write().await = Some(request.candidate_id.clone());
                true
            }
            Some(ref id) if id == &request.candidate_id => true,
            _ => false,
        };
        debug!(
            "vote request from {}: granted={}",
            request.candidate_id, vote_granted
        );
        Ok(VoteResponse {
            node_id: self.node.id().clone(),
            term: self.node.term(),
            vote_granted,
        })
    }
    async fn append_entries(&self, request: HeartbeatRequest) -> Result<HeartbeatResponse> {
        let current_term = self.node.term();
        if request.term < current_term {
            return Ok(HeartbeatResponse {
                node_id: self.node.id().clone(),
                term: current_term,
                success: false,
                metrics: self.node.metrics(),
            });
        }
        if request.term > current_term {
            self.node.set_term(request.term);
        }
        self.node.set_state(NodeState::Follower);
        self.node.set_leader(Some(request.leader_id.clone()));
        let mut commit_index = self.commit_index.write().await;
        if request.commit_index > *commit_index {
            *commit_index = request.commit_index;
        }
        Ok(HeartbeatResponse {
            node_id: self.node.id().clone(),
            term: self.node.term(),
            success: true,
            metrics: self.node.metrics(),
        })
    }
    fn state(&self) -> NodeState {
        self.node.state()
    }
    fn term(&self) -> u64 {
        self.node.term()
    }
    fn leader(&self) -> Option<NodeId> {
        self.node.leader()
    }
}
