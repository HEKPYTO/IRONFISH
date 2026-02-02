use crate::engine::{BestMove, UciInfo};
use crate::pool::EnginePool;
use chrono::Utc;
use ironfish_core::{
    AnalysisRequest, AnalysisResult, BestMoveRequest, BestMoveResponse, ChessPosition, Error,
    Evaluation, Move, PrincipalVariation, Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tracing::{debug, instrument};
pub struct AnalysisService {
    pool: Option<Arc<EnginePool>>,
    default_depth: u8,
    default_movetime: u64,
    analysis_timeout: Duration,
    mock_mode: bool,
}
impl AnalysisService {
    pub fn new(pool: Arc<EnginePool>) -> Self {
        Self {
            pool: Some(pool),
            default_depth: 20,
            default_movetime: 1000,
            analysis_timeout: Duration::from_secs(60),
            mock_mode: false,
        }
    }
    pub fn new_mock() -> Self {
        Self {
            pool: None,
            default_depth: 20,
            default_movetime: 1000,
            analysis_timeout: Duration::from_secs(60),
            mock_mode: true,
        }
    }
    pub fn with_default_depth(mut self, depth: u8) -> Self {
        self.default_depth = depth;
        self
    }
    pub fn with_default_movetime(mut self, ms: u64) -> Self {
        self.default_movetime = ms;
        self
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.analysis_timeout = timeout;
        self
    }
    #[instrument(skip(self), fields(id = %request.id))]
    pub async fn analyze(&self, request: AnalysisRequest) -> Result<AnalysisResult> {
        let position = ChessPosition::new(&request.fen);
        if !position.validate() {
            return Err(Error::InvalidFen(request.fen.clone()));
        }
        if self.mock_mode {
            return Ok(self.mock_analysis_result(&request));
        }
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| Error::Engine("no pool".into()))?;
        let pooled = pool.acquire().await?;
        let engine = pooled.engine();
        engine.ensure_ready().await?;
        engine.set_multipv(request.multipv.max(1)).await?;
        engine.set_position(&request.fen).await?;
        engine.go_depth(request.depth).await?;
        let result = timeout(
            self.analysis_timeout,
            self.collect_analysis(&request, engine),
        )
        .await
        .map_err(|_| Error::AnalysisTimeout)??;
        Ok(result)
    }
    async fn collect_analysis(
        &self,
        request: &AnalysisRequest,
        engine: &crate::engine::StockfishEngine,
    ) -> Result<AnalysisResult> {
        let mut pvs: HashMap<u8, (UciInfo, Vec<String>)> = HashMap::new();
        let mut final_info: Option<UciInfo> = None;
        let start = std::time::Instant::now();
        let best = loop {
            let line = engine.read_line().await?;
            let line = line.trim();
            if let Some(info) = UciInfo::parse(line) {
                let pv_idx = info.multipv.unwrap_or(1);
                if !info.pv.is_empty() {
                    pvs.insert(pv_idx, (info.clone(), info.pv.clone()));
                }
                final_info = Some(info);
            }
            if let Some(bm) = BestMove::parse(line) {
                break bm;
            }
        };
        let info = final_info.unwrap_or_default();
        let elapsed = start.elapsed();
        let best_move_parsed =
            Move::from_uci(&best.mv).ok_or_else(|| Error::Engine("invalid bestmove".into()))?;
        let ponder = best.ponder.as_ref().and_then(|p| Move::from_uci(p));
        let evaluation = if let Some(mate) = info.score_mate {
            Evaluation::mate(mate)
        } else {
            Evaluation::centipawns(info.score_cp.unwrap_or(0))
        };
        let mut principal_variations: Vec<PrincipalVariation> = pvs
            .into_iter()
            .map(|(rank, (pv_info, moves))| {
                let moves: Vec<Move> = moves.iter().filter_map(|m| Move::from_uci(m)).collect();
                let eval = if let Some(mate) = pv_info.score_mate {
                    Evaluation::mate(mate)
                } else {
                    Evaluation::centipawns(pv_info.score_cp.unwrap_or(0))
                };
                PrincipalVariation {
                    rank,
                    moves,
                    evaluation: eval,
                    depth: pv_info.depth.unwrap_or(0),
                }
            })
            .collect();
        principal_variations.sort_by_key(|pv| pv.rank);
        Ok(AnalysisResult {
            id: request.id,
            fen: request.fen.clone(),
            best_move: best_move_parsed,
            ponder,
            evaluation,
            principal_variations,
            depth_reached: info.depth.unwrap_or(request.depth),
            nodes_searched: info.nodes.unwrap_or(0),
            time_ms: elapsed.as_millis() as u64,
            completed_at: Utc::now(),
        })
    }
    #[instrument(skip(self))]
    pub async fn best_move(&self, request: BestMoveRequest) -> Result<BestMoveResponse> {
        let position = ChessPosition::new(&request.fen);
        if !position.validate() {
            return Err(Error::InvalidFen(request.fen.clone()));
        }
        if self.mock_mode {
            return Ok(self.mock_best_move_result());
        }
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| Error::Engine("no pool".into()))?;
        let pooled = pool.acquire().await?;
        let engine = pooled.engine();
        engine.ensure_ready().await?;
        engine.set_position(&request.fen).await?;
        let movetime = request.movetime.unwrap_or(self.default_movetime);
        engine.go_movetime(movetime).await?;
        let mut best_move: Option<BestMove> = None;
        let result = timeout(Duration::from_millis(movetime + 5000), async {
            loop {
                let line = engine.read_line().await?;
                if let Some(bm) = BestMove::parse(line.trim()) {
                    best_move = Some(bm);
                    break;
                }
            }
            Ok::<(), Error>(())
        })
        .await
        .map_err(|_| Error::AnalysisTimeout)?;
        result?;
        let best = best_move.ok_or_else(|| Error::Engine("no bestmove received".into()))?;
        let mv =
            Move::from_uci(&best.mv).ok_or_else(|| Error::Engine("invalid bestmove".into()))?;
        let ponder = best.ponder.as_ref().and_then(|p| Move::from_uci(p));
        debug!(best_move = %best.mv, "best move found");
        Ok(BestMoveResponse {
            best_move: mv,
            ponder,
        })
    }
    pub fn pool(&self) -> Option<&EnginePool> {
        self.pool.as_ref().map(|p| p.as_ref())
    }
    fn mock_analysis_result(&self, request: &AnalysisRequest) -> AnalysisResult {
        AnalysisResult {
            id: request.id,
            fen: request.fen.clone(),
            best_move: Move {
                from: "e2".to_string(),
                to: "e4".to_string(),
                promotion: None,
            },
            ponder: Some(Move {
                from: "e7".to_string(),
                to: "e5".to_string(),
                promotion: None,
            }),
            evaluation: Evaluation::centipawns(30),
            principal_variations: vec![PrincipalVariation {
                rank: 1,
                moves: vec![
                    Move {
                        from: "e2".to_string(),
                        to: "e4".to_string(),
                        promotion: None,
                    },
                    Move {
                        from: "e7".to_string(),
                        to: "e5".to_string(),
                        promotion: None,
                    },
                ],
                evaluation: Evaluation::centipawns(30),
                depth: request.depth,
            }],
            depth_reached: request.depth,
            nodes_searched: 10000,
            time_ms: 100,
            completed_at: Utc::now(),
        }
    }
    fn mock_best_move_result(&self) -> BestMoveResponse {
        BestMoveResponse {
            best_move: Move {
                from: "e2".to_string(),
                to: "e4".to_string(),
                promotion: None,
            },
            ponder: Some(Move {
                from: "e7".to_string(),
                to: "e5".to_string(),
                promotion: None,
            }),
        }
    }
}
