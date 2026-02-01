use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::debug;

use ironfish_core::{Error, LoadBalancer, NodeId, NodeMetrics, Result};

#[derive(Debug, Clone)]
pub struct LoadBalancerConfig {
    pub strategy: LoadBalanceStrategy,
    pub cpu_weight: f32,
    pub queue_weight: f32,
    pub latency_weight: f32,
    pub max_queue_depth: u32,
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            strategy: LoadBalanceStrategy::CpuAware,
            cpu_weight: 0.4,
            queue_weight: 0.3,
            latency_weight: 0.3,
            max_queue_depth: 100,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalanceStrategy {
    RoundRobin,
    LeastConnections,
    CpuAware,
}

struct NodeScore {
    metrics: NodeMetrics,
    healthy: bool,
    score: f64,
}

pub struct CpuAwareLoadBalancer {
    config: LoadBalancerConfig,
    nodes: Arc<RwLock<HashMap<NodeId, NodeScore>>>,
    round_robin_counter: AtomicUsize,
}

impl CpuAwareLoadBalancer {
    pub fn new(config: LoadBalancerConfig) -> Self {
        Self {
            config,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            round_robin_counter: AtomicUsize::new(0),
        }
    }

    pub async fn add_node(&self, node_id: NodeId) {
        let mut nodes = self.nodes.write().await;
        nodes.insert(
            node_id,
            NodeScore {
                metrics: NodeMetrics::default(),
                healthy: true,
                score: 1.0,
            },
        );
    }

    pub async fn remove_node(&self, node_id: &NodeId) {
        let mut nodes = self.nodes.write().await;
        nodes.remove(node_id);
    }

    fn calculate_score(&self, metrics: &NodeMetrics) -> f64 {
        let cpu_score = (1.0 - metrics.cpu_usage) as f64 * self.config.cpu_weight as f64;

        let queue_score =
            (1.0 / (metrics.queue_depth as f64 + 1.0)) * self.config.queue_weight as f64;

        let latency_score =
            (1.0 / (metrics.avg_latency_ms as f64 + 1.0)) * self.config.latency_weight as f64;

        cpu_score + queue_score + latency_score
    }

    async fn select_round_robin(&self, exclude: &[NodeId]) -> Result<NodeId> {
        let nodes = self.nodes.read().await;
        let available: Vec<_> = nodes
            .iter()
            .filter(|(id, score)| score.healthy && !exclude.contains(id))
            .collect();

        if available.is_empty() {
            return Err(Error::ClusterUnavailable);
        }

        let idx = self.round_robin_counter.fetch_add(1, Ordering::SeqCst) % available.len();
        Ok(available[idx].0.clone())
    }

    async fn select_least_connections(&self, exclude: &[NodeId]) -> Result<NodeId> {
        let nodes = self.nodes.read().await;

        nodes
            .iter()
            .filter(|(id, score)| score.healthy && !exclude.contains(id))
            .min_by_key(|(_, score)| score.metrics.active_analyses)
            .map(|(id, _)| id.clone())
            .ok_or(Error::ClusterUnavailable)
    }

    async fn select_cpu_aware(&self, exclude: &[NodeId]) -> Result<NodeId> {
        let nodes = self.nodes.read().await;

        nodes
            .iter()
            .filter(|(id, score)| score.healthy && !exclude.contains(id))
            .max_by(|(_, a), (_, b)| a.score.partial_cmp(&b.score).unwrap())
            .map(|(id, _)| id.clone())
            .ok_or(Error::ClusterUnavailable)
    }
}

#[async_trait]
impl LoadBalancer for CpuAwareLoadBalancer {
    async fn select_node(&self, exclude: &[NodeId]) -> Result<NodeId> {
        let result = match self.config.strategy {
            LoadBalanceStrategy::RoundRobin => self.select_round_robin(exclude).await,
            LoadBalanceStrategy::LeastConnections => self.select_least_connections(exclude).await,
            LoadBalanceStrategy::CpuAware => self.select_cpu_aware(exclude).await,
        };

        if let Ok(ref node_id) = result {
            debug!("selected node {} for request", node_id);
        }

        result
    }

    async fn update_metrics(&self, node_id: &NodeId, metrics: NodeMetrics) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if let Some(node_score) = nodes.get_mut(node_id) {
            let score = self.calculate_score(&metrics);
            node_score.metrics = metrics;
            node_score.score = score;

            debug!("updated metrics for node {}, score: {:.4}", node_id, score);
        }

        Ok(())
    }

    async fn mark_unhealthy(&self, node_id: &NodeId) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if let Some(node_score) = nodes.get_mut(node_id) {
            node_score.healthy = false;
            node_score.score = 0.0;
            debug!("marked node {} as unhealthy", node_id);
        }

        Ok(())
    }

    async fn mark_healthy(&self, node_id: &NodeId) -> Result<()> {
        let mut nodes = self.nodes.write().await;

        if let Some(node_score) = nodes.get_mut(node_id) {
            node_score.healthy = true;
            node_score.score = self.calculate_score(&node_score.metrics);
            debug!("marked node {} as healthy", node_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_balancer() {
        let config = LoadBalancerConfig::default();
        let lb = CpuAwareLoadBalancer::new(config);

        let node1 = NodeId::from_string("node1");
        let node2 = NodeId::from_string("node2");

        lb.add_node(node1.clone()).await;
        lb.add_node(node2.clone()).await;

        let selected = lb.select_node(&[]).await.unwrap();
        assert!(selected == node1 || selected == node2);

        lb.mark_unhealthy(&node1).await.unwrap();
        let selected = lb.select_node(&[]).await.unwrap();
        assert_eq!(selected, node2);
    }
}
