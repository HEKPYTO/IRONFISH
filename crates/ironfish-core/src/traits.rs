use async_trait::async_trait;

use crate::error::Result;
use crate::types::*;

#[async_trait]
pub trait AnalysisEngine: Send + Sync {
    async fn analyze(&self, request: AnalysisRequest) -> Result<AnalysisResult>;
    async fn best_move(&self, request: BestMoveRequest) -> Result<BestMoveResponse>;
    async fn stop(&self) -> Result<()>;
    fn is_ready(&self) -> bool;
}

#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn create(&self, token: ApiToken) -> Result<()>;
    async fn get(&self, id: &uuid::Uuid) -> Result<Option<ApiToken>>;
    async fn get_by_hash(&self, hash: &str) -> Result<Option<ApiToken>>;
    async fn update(&self, token: ApiToken) -> Result<()>;
    async fn delete(&self, id: &uuid::Uuid) -> Result<()>;
    async fn list(&self) -> Result<Vec<ApiToken>>;
    async fn revoke(&self, id: &uuid::Uuid) -> Result<()>;
}

#[async_trait]
pub trait ClusterDiscovery: Send + Sync {
    async fn discover(&self) -> Result<Vec<NodeInfo>>;
    async fn announce(&self, node: &NodeInfo) -> Result<()>;
    async fn withdraw(&self, node_id: &NodeId) -> Result<()>;
}

#[async_trait]
pub trait ConsensusProtocol: Send + Sync {
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn request_vote(&self, request: VoteRequest) -> Result<VoteResponse>;
    async fn append_entries(&self, request: HeartbeatRequest) -> Result<HeartbeatResponse>;
    fn state(&self) -> NodeState;
    fn term(&self) -> u64;
    fn leader(&self) -> Option<NodeId>;
}

#[async_trait]
pub trait LoadBalancer: Send + Sync {
    async fn select_node(&self, exclude: &[NodeId]) -> Result<NodeId>;
    async fn update_metrics(&self, node_id: &NodeId, metrics: NodeMetrics) -> Result<()>;
    async fn mark_unhealthy(&self, node_id: &NodeId) -> Result<()>;
    async fn mark_healthy(&self, node_id: &NodeId) -> Result<()>;
}

#[async_trait]
pub trait GossipProtocol: Send + Sync {
    async fn broadcast(&self, message: GossipMessage) -> Result<()>;
    async fn receive(&self) -> Result<GossipMessage>;
    async fn sync(&self, peer: &NodeId) -> Result<()>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GossipMessage {
    TokenCreated(ApiToken),
    TokenRevoked(uuid::Uuid),
    TokenUpdated(ApiToken),
    NodeJoined(NodeInfo),
    NodeLeft(NodeId),
    NodeMetrics(NodeId, NodeMetrics),
}
