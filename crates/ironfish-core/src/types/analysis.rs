use super::Move;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub id: Uuid,
    pub fen: String,
    pub depth: u8,
    pub multipv: u8,
    pub movetime: Option<u64>,
}
impl AnalysisRequest {
    pub fn new(fen: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            fen: fen.into(),
            depth: 20,
            multipv: 1,
            movetime: None,
        }
    }
    pub fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }
    pub fn with_multipv(mut self, multipv: u8) -> Self {
        self.multipv = multipv.max(1);
        self
    }
    pub fn with_movetime(mut self, ms: u64) -> Self {
        self.movetime = Some(ms);
        self
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub id: Uuid,
    pub fen: String,
    pub best_move: Move,
    pub ponder: Option<Move>,
    pub evaluation: Evaluation,
    pub principal_variations: Vec<PrincipalVariation>,
    pub depth_reached: u8,
    pub nodes_searched: u64,
    pub time_ms: u64,
    pub completed_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub score_type: ScoreType,
    pub value: i32,
}
impl Evaluation {
    pub fn centipawns(cp: i32) -> Self {
        Self {
            score_type: ScoreType::Centipawns,
            value: cp,
        }
    }
    pub fn mate(moves: i32) -> Self {
        Self {
            score_type: ScoreType::Mate,
            value: moves,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScoreType {
    Centipawns,
    Mate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalVariation {
    pub rank: u8,
    pub moves: Vec<Move>,
    pub evaluation: Evaluation,
    pub depth: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisProgress {
    pub id: Uuid,
    pub current_depth: u8,
    pub target_depth: u8,
    pub current_move: Option<Move>,
    pub nodes_per_second: u64,
    pub hash_full: u16,
    pub evaluation: Option<Evaluation>,
    pub principal_variations: Vec<PrincipalVariation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestMoveRequest {
    pub fen: String,
    pub movetime: Option<u64>,
}
impl BestMoveRequest {
    pub fn new(fen: impl Into<String>) -> Self {
        Self {
            fen: fen.into(),
            movetime: Some(1000),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestMoveResponse {
    pub best_move: Move,
    pub ponder: Option<Move>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_analysis_request_builder() {
        let req = AnalysisRequest::new("startpos")
            .with_depth(15)
            .with_multipv(3)
            .with_movetime(5000);
        assert_eq!(req.fen, "startpos");
        assert_eq!(req.depth, 15);
        assert_eq!(req.multipv, 3);
        assert_eq!(req.movetime, Some(5000));
    }
    #[test]
    fn test_analysis_request_defaults() {
        let req = AnalysisRequest::new("fen");
        assert_eq!(req.depth, 20);
        assert_eq!(req.multipv, 1);
        assert_eq!(req.movetime, None);
    }
    #[test]
    fn test_multipv_minimum() {
        let req = AnalysisRequest::new("fen").with_multipv(0);
        assert_eq!(req.multipv, 1);
    }
    #[test]
    fn test_evaluation_centipawns() {
        let eval = Evaluation::centipawns(150);
        assert_eq!(eval.score_type, ScoreType::Centipawns);
        assert_eq!(eval.value, 150);
    }
    #[test]
    fn test_evaluation_mate() {
        let eval = Evaluation::mate(3);
        assert_eq!(eval.score_type, ScoreType::Mate);
        assert_eq!(eval.value, 3);
    }
    #[test]
    fn test_best_move_request() {
        let req = BestMoveRequest::new("startpos");
        assert_eq!(req.fen, "startpos");
        assert_eq!(req.movetime, Some(1000));
    }
}
