use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid FEN: {0}")]
    InvalidFen(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("engine pool exhausted")]
    PoolExhausted,

    #[error("analysis timeout")]
    AnalysisTimeout,

    #[error("invalid token")]
    InvalidToken,

    #[error("token expired")]
    TokenExpired,

    #[error("token not found")]
    TokenNotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("rate limit exceeded")]
    RateLimitExceeded,

    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("not leader")]
    NotLeader,

    #[error("cluster unavailable")]
    ClusterUnavailable,

    #[error("consensus error: {0}")]
    Consensus(String),

    #[error("discovery error: {0}")]
    Discovery(String),

    #[error("gossip error: {0}")]
    Gossip(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
