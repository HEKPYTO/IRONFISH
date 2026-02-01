use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub node: NodeConfig,

    #[serde(default)]
    pub stockfish: StockfishConfig,

    #[serde(default)]
    pub cluster: ClusterConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub load_balancer: LoadBalancerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    #[serde(default = "default_node_id")]
    pub id: String,

    #[serde(default = "default_bind_address")]
    pub bind_address: SocketAddr,

    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    #[serde(default = "default_priority")]
    pub priority: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct StockfishConfig {
    #[serde(default = "default_stockfish_path")]
    pub binary_path: String,

    #[serde(default = "default_pool_size")]
    pub pool_size: usize,

    #[serde(default = "default_depth")]
    pub default_depth: u8,

    #[serde(default = "default_multipv")]
    pub default_multipv: u8,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ClusterConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,

    #[serde(default = "default_election_timeout")]
    pub election_timeout_ms: u64,

    #[serde(default = "default_gossip_interval")]
    pub gossip_interval_ms: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default)]
    pub static_peers: Vec<String>,

    #[serde(default)]
    pub seed_nodes: Vec<String>,

    #[serde(default = "default_true")]
    pub multicast_enabled: bool,

    #[serde(default = "default_multicast_group")]
    pub multicast_group: String,

    #[serde(default = "default_multicast_port")]
    pub multicast_port: u16,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_token_ttl")]
    pub token_ttl_days: u32,

    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,

    #[serde(default = "default_token_secret")]
    pub token_secret: String,
}

fn default_token_secret() -> String {
    let secret = std::env::var("IRONFISH_TOKEN_SECRET")
        .unwrap_or_else(|_| "default-dev-secret-change-in-production".to_string());

    if !cfg!(debug_assertions) && secret == "default-dev-secret-change-in-production" {
        panic!("FATAL: You are running in RELEASE mode with the default insecure TOKEN_SECRET.");
    }
    secret
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct LoadBalancerConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,

    #[serde(default = "default_cpu_weight")]
    pub cpu_weight: f32,

    #[serde(default = "default_queue_weight")]
    pub queue_weight: f32,

    #[serde(default = "default_latency_weight")]
    pub latency_weight: f32,
}

fn default_node_id() -> String {
    std::env::var("IRONFISH_NODE_ID").unwrap_or_else(|_| "auto".to_string())
}

fn default_bind_address() -> SocketAddr {
    std::env::var("IRONFISH_BIND_ADDRESS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:8080".parse().unwrap())
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/ironfish")
}

fn default_priority() -> u32 {
    100
}

fn default_stockfish_path() -> String {
    std::env::var("STOCKFISH_PATH").unwrap_or_else(|_| "/usr/local/bin/stockfish".to_string())
}

fn default_pool_size() -> usize {
    4
}

fn default_depth() -> u8 {
    20
}

fn default_multipv() -> u8 {
    3
}

fn default_true() -> bool {
    true
}

fn default_heartbeat_interval() -> u64 {
    1000
}

fn default_election_timeout() -> u64 {
    5000
}

fn default_gossip_interval() -> u64 {
    5000
}

fn default_multicast_group() -> String {
    "239.255.42.98".to_string()
}

fn default_multicast_port() -> u16 {
    7878
}

fn default_token_ttl() -> u32 {
    365
}

fn default_rate_limit() -> u32 {
    100
}

fn default_strategy() -> String {
    "cpu_aware".to_string()
}

fn default_cpu_weight() -> f32 {
    0.4
}

fn default_queue_weight() -> f32 {
    0.3
}

fn default_latency_weight() -> f32 {
    0.3
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: default_node_id(),
            bind_address: default_bind_address(),
            data_dir: default_data_dir(),
            priority: default_priority(),
        }
    }
}

impl Default for StockfishConfig {
    fn default() -> Self {
        Self {
            binary_path: default_stockfish_path(),
            pool_size: default_pool_size(),
            default_depth: default_depth(),
            default_multipv: default_multipv(),
        }
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            heartbeat_interval_ms: default_heartbeat_interval(),
            election_timeout_ms: default_election_timeout(),
            gossip_interval_ms: default_gossip_interval(),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        let static_peers = std::env::var("IRONFISH_CLUSTER_PEERS")
            .map(|s| s.split(',').map(String::from).collect())
            .unwrap_or_default();

        Self {
            static_peers,
            seed_nodes: Vec::new(),
            multicast_enabled: true,
            multicast_group: default_multicast_group(),
            multicast_port: default_multicast_port(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            token_ttl_days: default_token_ttl(),
            rate_limit_per_minute: default_rate_limit(),
            token_secret: default_token_secret(),
        }
    }
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            cpu_weight: default_cpu_weight(),
            queue_weight: default_queue_weight(),
            latency_weight: default_latency_weight(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path =
            std::env::var("IRONFISH_CONFIG").unwrap_or_else(|_| "config/default.toml".to_string());

        if std::path::Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }
}
