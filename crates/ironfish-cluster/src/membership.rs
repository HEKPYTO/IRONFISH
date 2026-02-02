use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use ironfish_core::{
    ClusterStatus, JoinRequest, JoinResponse, NodeId, NodeInfo, NodeState, NodeStatus, Result,
};
use crate::node::SharedNode;
pub struct MembershipManager {
    local_node: SharedNode,
    members: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
}
impl MembershipManager {
    pub fn new(local_node: SharedNode) -> Self {
        Self {
            local_node,
            members: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub async fn join(&self, request: JoinRequest) -> Result<JoinResponse> {
        let is_leader = self.local_node.is_leader();
        if !is_leader {
            return Ok(JoinResponse {
                accepted: false,
                leader_id: self.local_node.leader(),
                members: Vec::new(),
                term: self.local_node.term(),
            });
        }
        let mut members = self.members.write().await;
        if members.contains_key(&request.node_info.id) {
            warn!("node {} already in cluster", request.node_info.id);
        }
        members.insert(request.node_info.id.clone(), request.node_info.clone());
        info!("node {} joined cluster", request.node_info.id);
        let member_list: Vec<NodeInfo> = members.values().cloned().collect();
        Ok(JoinResponse {
            accepted: true,
            leader_id: Some(self.local_node.id().clone()),
            members: member_list,
            term: self.local_node.term(),
        })
    }
    pub async fn leave(&self, node_id: &NodeId) -> Result<()> {
        let mut members = self.members.write().await;
        members.remove(node_id);
        info!("node {} left cluster", node_id);
        Ok(())
    }
    pub async fn add_member(&self, node: NodeInfo) {
        let mut members = self.members.write().await;
        debug!("adding member {}", node.id);
        members.insert(node.id.clone(), node);
    }
    pub async fn remove_member(&self, node_id: &NodeId) {
        let mut members = self.members.write().await;
        debug!("removing member {}", node_id);
        members.remove(node_id);
    }
    pub async fn get_member(&self, node_id: &NodeId) -> Option<NodeInfo> {
        let members = self.members.read().await;
        members.get(node_id).cloned()
    }
    pub async fn list_members(&self) -> Vec<NodeInfo> {
        let members = self.members.read().await;
        members.values().cloned().collect()
    }
    pub async fn member_count(&self) -> usize {
        self.members.read().await.len() + 1
    }
    pub async fn cluster_status(&self) -> ClusterStatus {
        let members = self.members.read().await;
        let local_status = self.local_node.status(members.len() + 1);
        let mut nodes = vec![local_status];
        for (_, info) in members.iter() {
            nodes.push(NodeStatus {
                info: info.clone(),
                state: NodeState::Follower,
                leader_id: self.local_node.leader(),
                term: self.local_node.term(),
                cluster_size: members.len() + 1,
                uptime_seconds: 0,
            });
        }
        ClusterStatus {
            nodes,
            leader: self.local_node.leader(),
            term: self.local_node.term(),
            healthy: true,
        }
    }
    pub async fn is_member(&self, node_id: &NodeId) -> bool {
        if node_id == self.local_node.id() {
            return true;
        }
        let members = self.members.read().await;
        members.contains_key(node_id)
    }
}
