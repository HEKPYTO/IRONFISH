use chrono::Utc;
use ironfish_cluster::{
    discovery::StaticDiscovery, CpuAwareLoadBalancer, GossipService, LoadBalancerConfig,
    MembershipManager, Node, NodeConfig,
};
use ironfish_core::{ClusterDiscovery, LoadBalancer, NodeId, NodeInfo, NodeMetrics};
use std::sync::Arc;
#[tokio::test]
async fn test_node_creation_with_auto_id() {
    let config = NodeConfig::default();
    let node = Node::new(config);
    assert!(!node.id().0.is_empty());
}
#[tokio::test]
async fn test_node_creation_with_custom_id() {
    let config = NodeConfig {
        id: Some("custom-node-id".to_string()),
        bind_address: "127.0.0.1:8080".parse().unwrap(),
        priority: 150,
        version: "1.0.0".to_string(),
    };
    let node = Node::new(config);
    assert_eq!(node.id().0, "custom-node-id");
    assert_eq!(node.priority(), 150);
}
#[tokio::test]
async fn test_membership_add_and_remove() {
    let node = Arc::new(Node::new(NodeConfig::default()));
    let manager = MembershipManager::new(node);
    let peer = NodeInfo {
        id: NodeId::from_string("peer-1"),
        address: "192.168.1.10:8080".parse().unwrap(),
        priority: 100,
        started_at: Utc::now(),
        version: "1.0.0".to_string(),
    };
    manager.add_member(peer.clone()).await;
    let peers = manager.list_members().await;
    assert_eq!(peers.len(), 1);
    manager.remove_member(&peer.id).await;
    let peers = manager.list_members().await;
    assert_eq!(peers.len(), 0);
}
#[tokio::test]
async fn test_static_discovery_valid_peers() {
    let peers = vec![
        "192.168.1.10:8080".to_string(),
        "192.168.1.11:8080".to_string(),
    ];
    let discovery = StaticDiscovery::new(peers);
    let nodes = discovery.discover().await.unwrap();
    assert_eq!(nodes.len(), 2);
}
#[tokio::test]
async fn test_static_discovery_invalid_peers() {
    let peers = vec!["invalid-address".to_string()];
    let discovery = StaticDiscovery::new(peers);
    let nodes = discovery.discover().await.unwrap();
    assert!(nodes.is_empty());
}
#[tokio::test]
async fn test_gossip_service_peer_management() {
    let node_id = NodeId::from_string("gossip-test");
    let service = GossipService::new(node_id);
    let peer = NodeInfo {
        id: NodeId::from_string("peer-1"),
        address: "192.168.1.10:8080".parse().unwrap(),
        priority: 100,
        started_at: Utc::now(),
        version: "1.0.0".to_string(),
    };
    service.add_peer(peer.clone()).await;
    service.add_peer(peer.clone()).await;
    service.remove_peer(&peer.id).await;
}
#[tokio::test]
async fn test_load_balancer_selection() {
    let config = LoadBalancerConfig::default();
    let balancer = CpuAwareLoadBalancer::new(config);
    let node1 = NodeId::from_string("node-1");
    let node2 = NodeId::from_string("node-2");
    balancer.add_node(node1.clone()).await;
    balancer.add_node(node2.clone()).await;
    let selected = balancer.select_node(&[]).await;
    assert!(selected.is_ok());
}
#[tokio::test]
async fn test_load_balancer_update_metrics() {
    let config = LoadBalancerConfig::default();
    let balancer = CpuAwareLoadBalancer::new(config);
    let node_id = NodeId::from_string("node-1");
    balancer.add_node(node_id.clone()).await;
    let updated_metrics = NodeMetrics {
        cpu_usage: 0.8,
        active_analyses: 5,
        ..Default::default()
    };
    balancer
        .update_metrics(&node_id, updated_metrics)
        .await
        .unwrap();
    let selected = balancer.select_node(&[]).await;
    assert!(selected.is_ok());
}
#[tokio::test]
async fn test_load_balancer_empty() {
    let config = LoadBalancerConfig::default();
    let balancer = CpuAwareLoadBalancer::new(config);
    let selected = balancer.select_node(&[]).await;
    assert!(selected.is_err());
}
#[tokio::test]
async fn test_load_balancer_exclude_node() {
    let config = LoadBalancerConfig::default();
    let balancer = CpuAwareLoadBalancer::new(config);
    let node1 = NodeId::from_string("node-1");
    let node2 = NodeId::from_string("node-2");
    balancer.add_node(node1.clone()).await;
    balancer.add_node(node2.clone()).await;
    let selected = balancer
        .select_node(std::slice::from_ref(&node1))
        .await
        .unwrap();
    assert_eq!(selected, node2);
}
#[tokio::test]
async fn test_load_balancer_mark_unhealthy() {
    let config = LoadBalancerConfig::default();
    let balancer = CpuAwareLoadBalancer::new(config);
    let node1 = NodeId::from_string("node-1");
    let node2 = NodeId::from_string("node-2");
    balancer.add_node(node1.clone()).await;
    balancer.add_node(node2.clone()).await;
    balancer.mark_unhealthy(&node1).await.unwrap();
    let selected = balancer.select_node(&[]).await.unwrap();
    assert_eq!(selected, node2);
}
#[tokio::test]
async fn test_node_state_transitions() {
    use ironfish_core::NodeState;
    let node = Node::new(NodeConfig::default());
    assert_eq!(node.state(), NodeState::Starting);
    node.set_state(NodeState::Follower);
    assert_eq!(node.state(), NodeState::Follower);
    node.set_state(NodeState::Candidate);
    assert_eq!(node.state(), NodeState::Candidate);
    node.set_state(NodeState::Leader);
    assert!(node.is_leader());
}
#[tokio::test]
async fn test_node_term_increment() {
    let node = Node::new(NodeConfig::default());
    assert_eq!(node.term(), 0);
    let new_term = node.increment_term();
    assert_eq!(new_term, 1);
    assert_eq!(node.term(), 1);
    node.set_term(10);
    assert_eq!(node.term(), 10);
}
#[tokio::test]
async fn test_membership_is_member() {
    let node = Arc::new(Node::new(NodeConfig::default()));
    let local_id = node.id().clone();
    let manager = MembershipManager::new(node);
    assert!(manager.is_member(&local_id).await);
    let peer_id = NodeId::from_string("peer-1");
    assert!(!manager.is_member(&peer_id).await);
    let peer = NodeInfo {
        id: peer_id.clone(),
        address: "192.168.1.10:8080".parse().unwrap(),
        priority: 100,
        started_at: Utc::now(),
        version: "1.0.0".to_string(),
    };
    manager.add_member(peer).await;
    assert!(manager.is_member(&peer_id).await);
}
#[tokio::test]
async fn test_membership_cluster_status() {
    let node = Arc::new(Node::new(NodeConfig::default()));
    let manager = MembershipManager::new(node);
    let status = manager.cluster_status().await;
    assert_eq!(status.nodes.len(), 1);
    assert!(status.healthy);
}
