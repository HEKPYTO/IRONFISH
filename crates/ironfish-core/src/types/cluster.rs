use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub String);
impl NodeId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}
impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: NodeId,
    pub address: SocketAddr,
    pub priority: u32,
    pub started_at: DateTime<Utc>,
    pub version: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeState {
    Starting,
    Joining,
    Follower,
    Candidate,
    Leader,
    Leaving,
    Dead,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub info: NodeInfo,
    pub state: NodeState,
    pub leader_id: Option<NodeId>,
    pub term: u64,
    pub cluster_size: usize,
    pub uptime_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub active_analyses: u32,
    pub queue_depth: u32,
    pub avg_latency_ms: u64,
    pub total_requests: u64,
    pub engines_available: u32,
    pub engines_total: u32,
}
impl Default for NodeMetrics {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            active_analyses: 0,
            queue_depth: 0,
            avg_latency_ms: 0,
            total_requests: 0,
            engines_available: 0,
            engines_total: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub nodes: Vec<NodeStatus>,
    pub leader: Option<NodeId>,
    pub term: u64,
    pub healthy: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub node_info: NodeInfo,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResponse {
    pub accepted: bool,
    pub leader_id: Option<NodeId>,
    pub members: Vec<NodeInfo>,
    pub term: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub leader_id: NodeId,
    pub term: u64,
    pub commit_index: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub node_id: NodeId,
    pub term: u64,
    pub success: bool,
    pub metrics: NodeMetrics,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRequest {
    pub candidate_id: NodeId,
    pub term: u64,
    pub priority: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResponse {
    pub node_id: NodeId,
    pub term: u64,
    pub vote_granted: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_node_id_generate() {
        let id1 = NodeId::generate();
        let id2 = NodeId::generate();
        assert_ne!(id1, id2);
    }
    #[test]
    fn test_node_id_from_string() {
        let id = NodeId::from_string("test-node");
        assert_eq!(id.0, "test-node");
        assert_eq!(id.to_string(), "test-node");
    }
    #[test]
    fn test_node_id_equality() {
        let id1 = NodeId::from_string("node1");
        let id2 = NodeId::from_string("node1");
        let id3 = NodeId::from_string("node2");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
    #[test]
    fn test_node_state_variants() {
        let states = [
            NodeState::Starting,
            NodeState::Joining,
            NodeState::Follower,
            NodeState::Candidate,
            NodeState::Leader,
            NodeState::Leaving,
            NodeState::Dead,
        ];
        for state in states {
            assert_eq!(state, state);
        }
    }
    #[test]
    fn test_node_metrics_default() {
        let metrics = NodeMetrics::default();
        assert_eq!(metrics.cpu_usage, 0.0);
        assert_eq!(metrics.memory_usage, 0.0);
        assert_eq!(metrics.active_analyses, 0);
        assert_eq!(metrics.queue_depth, 0);
    }
    #[test]
    fn test_node_info_creation() {
        let info = NodeInfo {
            id: NodeId::from_string("test"),
            address: "127.0.0.1:8080".parse().unwrap(),
            priority: 100,
            started_at: Utc::now(),
            version: "1.0.0".to_string(),
        };
        assert_eq!(info.id.0, "test");
        assert_eq!(info.priority, 100);
    }
    #[test]
    fn test_cluster_status() {
        let status = ClusterStatus {
            nodes: vec![],
            leader: Some(NodeId::from_string("leader")),
            term: 5,
            healthy: true,
        };
        assert!(status.healthy);
        assert_eq!(status.term, 5);
        assert!(status.leader.is_some());
    }
    #[test]
    fn test_vote_request() {
        let req = VoteRequest {
            candidate_id: NodeId::from_string("candidate"),
            term: 10,
            priority: 150,
        };
        assert_eq!(req.term, 10);
        assert_eq!(req.priority, 150);
    }
    #[test]
    fn test_heartbeat_request() {
        let req = HeartbeatRequest {
            leader_id: NodeId::from_string("leader"),
            term: 5,
            commit_index: 100,
        };
        assert_eq!(req.term, 5);
        assert_eq!(req.commit_index, 100);
    }
}
