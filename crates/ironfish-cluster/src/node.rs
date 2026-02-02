use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use chrono::Utc;
use ironfish_core::{NodeId, NodeInfo, NodeMetrics, NodeState, NodeStatus};
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub id: Option<String>,
    pub bind_address: SocketAddr,
    pub priority: u32,
    pub version: String,
}
impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: None,
            bind_address: "0.0.0.0:8080".parse().unwrap(),
            priority: 100,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}
pub struct Node {
    info: NodeInfo,
    state: RwLock<NodeState>,
    leader_id: RwLock<Option<NodeId>>,
    term: AtomicU64,
    metrics: RwLock<NodeMetrics>,
    started_at: chrono::DateTime<Utc>,
}
impl Node {
    pub fn new(config: NodeConfig) -> Self {
        let id = config
            .id
            .map(NodeId::from_string)
            .unwrap_or_else(NodeId::generate);
        let started_at = Utc::now();
        let info = NodeInfo {
            id,
            address: config.bind_address,
            priority: config.priority,
            started_at,
            version: config.version,
        };
        Self {
            info,
            state: RwLock::new(NodeState::Starting),
            leader_id: RwLock::new(None),
            term: AtomicU64::new(0),
            metrics: RwLock::new(NodeMetrics::default()),
            started_at,
        }
    }
    pub fn info(&self) -> &NodeInfo {
        &self.info
    }
    pub fn id(&self) -> &NodeId {
        &self.info.id
    }
    pub fn state(&self) -> NodeState {
        *self.state.read().unwrap()
    }
    pub fn set_state(&self, state: NodeState) {
        *self.state.write().unwrap() = state;
    }
    pub fn leader(&self) -> Option<NodeId> {
        self.leader_id.read().unwrap().clone()
    }
    pub fn set_leader(&self, leader: Option<NodeId>) {
        *self.leader_id.write().unwrap() = leader;
    }
    pub fn term(&self) -> u64 {
        self.term.load(Ordering::SeqCst)
    }
    pub fn set_term(&self, term: u64) {
        self.term.store(term, Ordering::SeqCst);
    }
    pub fn increment_term(&self) -> u64 {
        self.term.fetch_add(1, Ordering::SeqCst) + 1
    }
    pub fn metrics(&self) -> NodeMetrics {
        self.metrics.read().unwrap().clone()
    }
    pub fn update_metrics(&self, metrics: NodeMetrics) {
        *self.metrics.write().unwrap() = metrics;
    }
    pub fn status(&self, cluster_size: usize) -> NodeStatus {
        let uptime = (Utc::now() - self.started_at).num_seconds() as u64;
        NodeStatus {
            info: self.info.clone(),
            state: self.state(),
            leader_id: self.leader(),
            term: self.term(),
            cluster_size,
            uptime_seconds: uptime,
        }
    }
    pub fn is_leader(&self) -> bool {
        matches!(self.state(), NodeState::Leader)
    }
    pub fn priority(&self) -> u32 {
        self.info.priority
    }
}
impl Clone for Node {
    fn clone(&self) -> Self {
        Self {
            info: self.info.clone(),
            state: RwLock::new(*self.state.read().unwrap()),
            leader_id: RwLock::new(self.leader_id.read().unwrap().clone()),
            term: AtomicU64::new(self.term.load(Ordering::SeqCst)),
            metrics: RwLock::new(self.metrics.read().unwrap().clone()),
            started_at: self.started_at,
        }
    }
}
pub type SharedNode = Arc<Node>;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_node_config_default() {
        let config = NodeConfig::default();
        assert!(config.id.is_none());
        assert_eq!(config.priority, 100);
    }
    #[test]
    fn test_node_creation() {
        let config = NodeConfig {
            id: Some("test-node".to_string()),
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            priority: 150,
            version: "1.0.0".to_string(),
        };
        let node = Node::new(config);
        assert_eq!(node.id().0, "test-node");
        assert_eq!(node.priority(), 150);
    }
    #[test]
    fn test_node_auto_id() {
        let config = NodeConfig::default();
        let node = Node::new(config);
        assert!(!node.id().0.is_empty());
    }
    #[test]
    fn test_node_state() {
        let node = Node::new(NodeConfig::default());
        assert_eq!(node.state(), NodeState::Starting);
        node.set_state(NodeState::Follower);
        assert_eq!(node.state(), NodeState::Follower);
        node.set_state(NodeState::Leader);
        assert!(node.is_leader());
    }
    #[test]
    fn test_node_leader() {
        let node = Node::new(NodeConfig::default());
        assert!(node.leader().is_none());
        let leader_id = NodeId::from_string("leader-1");
        node.set_leader(Some(leader_id.clone()));
        assert_eq!(node.leader(), Some(leader_id));
    }
    #[test]
    fn test_node_term() {
        let node = Node::new(NodeConfig::default());
        assert_eq!(node.term(), 0);
        node.set_term(5);
        assert_eq!(node.term(), 5);
        let new_term = node.increment_term();
        assert_eq!(new_term, 6);
        assert_eq!(node.term(), 6);
    }
    #[test]
    fn test_node_metrics() {
        let node = Node::new(NodeConfig::default());
        let initial = node.metrics();
        assert_eq!(initial.cpu_usage, 0.0);
        let new_metrics = NodeMetrics {
            cpu_usage: 0.5,
            active_analyses: 3,
            ..Default::default()
        };
        node.update_metrics(new_metrics);
        let updated = node.metrics();
        assert_eq!(updated.cpu_usage, 0.5);
        assert_eq!(updated.active_analyses, 3);
    }
    #[test]
    fn test_node_status() {
        let node = Node::new(NodeConfig::default());
        node.set_state(NodeState::Follower);
        node.set_term(10);
        let status = node.status(5);
        assert_eq!(status.state, NodeState::Follower);
        assert_eq!(status.term, 10);
        assert_eq!(status.cluster_size, 5);
    }
}
