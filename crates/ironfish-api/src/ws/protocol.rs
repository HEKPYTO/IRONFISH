use ironfish_core::{AnalysisResult, BestMoveResponse, Evaluation, PrincipalVariation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Auth {
        id: String,
        token: String,
    },
    Analyze {
        id: String,
        fen: String,
        #[serde(default = "default_depth")]
        depth: u8,
        #[serde(default = "default_multipv")]
        multipv: u8,
        movetime: Option<u64>,
    },
    Cancel {
        id: String,
        analysis_id: Uuid,
    },
    Bestmove {
        id: String,
        fen: String,
        movetime: Option<u64>,
    },
    Subscribe {
        id: String,
        topics: Vec<String>,
    },
    Unsubscribe {
        id: String,
        topics: Vec<String>,
    },
    Ping {
        id: String,
    },
}

fn default_depth() -> u8 {
    20
}

fn default_multipv() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    AuthResult {
        id: String,
        success: bool,
        error: Option<String>,
    },
    AnalysisProgress {
        analysis_id: Uuid,
        current_depth: u8,
        target_depth: u8,
        evaluation: Option<Evaluation>,
        principal_variations: Vec<PrincipalVariation>,
        nodes_per_second: u64,
    },
    AnalysisComplete {
        id: String,
        result: AnalysisResult,
    },
    AnalysisCancelled {
        analysis_id: Uuid,
    },
    BestmoveResult {
        id: String,
        result: BestMoveResponse,
    },
    ClusterEvent {
        event: serde_json::Value,
    },
    Subscribed {
        id: String,
        topics: Vec<String>,
    },
    Error {
        id: Option<String>,
        code: u16,
        message: String,
    },
    Pong {
        id: String,
    },
}
