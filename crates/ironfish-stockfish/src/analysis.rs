use crate::engine::{BestMove, UciInfo};
use crate::pool::EnginePool;
use chrono::Utc;
use ironfish_core::{
    AnalysisProgress, AnalysisRequest, AnalysisResult, BestMoveRequest, BestMoveResponse,
    ChessPosition, Error, Evaluation, Move, PrincipalVariation, Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;
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
    async fn stop_and_drain(engine: &crate::engine::StockfishEngine) {
        let _ = engine.stop().await;
        let drain_timeout = Duration::from_secs(10);
        let _ = timeout(drain_timeout, async {
            while let Ok(line) = engine.read_line().await {
                if BestMove::parse(line.trim()).is_some() {
                    break;
                }
            }
        })
        .await;
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
        match timeout(
            self.analysis_timeout,
            self.collect_analysis(&request, engine),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                Self::stop_and_drain(engine).await;
                Err(Error::AnalysisTimeout)
            }
        }
    }
    pub async fn analyze_streaming(
        &self,
        request: AnalysisRequest,
        progress_tx: mpsc::Sender<AnalysisProgress>,
        cancel: CancellationToken,
    ) -> Result<AnalysisResult> {
        let position = ChessPosition::new(&request.fen);
        if !position.validate() {
            return Err(Error::InvalidFen(request.fen.clone()));
        }
        if self.mock_mode {
            return self
                .mock_streaming_analysis(&request, progress_tx, cancel)
                .await;
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
        match timeout(
            self.analysis_timeout,
            self.collect_analysis_streaming(&request, engine, progress_tx, cancel),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                Self::stop_and_drain(engine).await;
                Err(Error::AnalysisTimeout)
            }
        }
    }

    async fn collect_analysis_streaming(
        &self,
        request: &AnalysisRequest,
        engine: &crate::engine::StockfishEngine,
        progress_tx: mpsc::Sender<AnalysisProgress>,
        cancel: CancellationToken,
    ) -> Result<AnalysisResult> {
        let mut pvs: HashMap<u8, (UciInfo, Vec<String>)> = HashMap::new();
        let mut final_info: Option<UciInfo> = None;
        let start = std::time::Instant::now();
        let best = loop {
            if cancel.is_cancelled() {
                let _ = engine.stop().await;
                loop {
                    let line = engine.read_line().await?;
                    if BestMove::parse(line.trim()).is_some() {
                        break;
                    }
                }
                return Err(Error::AnalysisCancelled);
            }
            let line = engine.read_line().await?;
            let line = line.trim().to_string();
            if let Some(info) = UciInfo::parse(&line) {
                let pv_idx = info.multipv.unwrap_or(1);
                if !info.pv.is_empty() {
                    pvs.insert(pv_idx, (info.clone(), info.pv.clone()));
                }
                if !info.pv.is_empty() || info.score_cp.is_some() || info.score_mate.is_some() {
                    let eval = if let Some(mate) = info.score_mate {
                        Some(Evaluation::mate(mate))
                    } else {
                        info.score_cp.map(Evaluation::centipawns)
                    };
                    let current_pvs: Vec<PrincipalVariation> = pvs
                        .iter()
                        .map(|(rank, (pv_info, moves))| {
                            let moves: Vec<Move> =
                                moves.iter().filter_map(|m| Move::from_uci(m)).collect();
                            let pv_eval = if let Some(mate) = pv_info.score_mate {
                                Evaluation::mate(mate)
                            } else {
                                Evaluation::centipawns(pv_info.score_cp.unwrap_or(0))
                            };
                            PrincipalVariation {
                                rank: *rank,
                                moves,
                                evaluation: pv_eval,
                                depth: pv_info.depth.unwrap_or(0),
                            }
                        })
                        .collect();
                    let progress = AnalysisProgress {
                        id: request.id,
                        current_depth: info.depth.unwrap_or(0),
                        target_depth: request.depth,
                        current_move: info.currmove.as_ref().and_then(|m| Move::from_uci(m)),
                        nodes_per_second: info.nps.unwrap_or(0),
                        hash_full: info.hashfull.unwrap_or(0),
                        evaluation: eval,
                        principal_variations: current_pvs,
                    };
                    let _ = progress_tx.try_send(progress);
                }
                final_info = Some(info);
            }
            if let Some(bm) = BestMove::parse(&line) {
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

    async fn mock_streaming_analysis(
        &self,
        request: &AnalysisRequest,
        progress_tx: mpsc::Sender<AnalysisProgress>,
        cancel: CancellationToken,
    ) -> Result<AnalysisResult> {
        for depth in [5, 10, 15, request.depth] {
            if cancel.is_cancelled() {
                return Err(Error::AnalysisCancelled);
            }
            let progress = AnalysisProgress {
                id: request.id,
                current_depth: depth,
                target_depth: request.depth,
                current_move: Some(Move {
                    from: "e2".to_string(),
                    to: "e4".to_string(),
                    promotion: None,
                }),
                nodes_per_second: 500000,
                hash_full: 100,
                evaluation: Some(Evaluation::centipawns(30)),
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
                    depth,
                }],
            };
            let _ = progress_tx.try_send(progress);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(self.mock_analysis_result(request))
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
        let search_result = timeout(Duration::from_millis(movetime + 5000), async {
            loop {
                let line = engine.read_line().await?;
                if let Some(bm) = BestMove::parse(line.trim()) {
                    best_move = Some(bm);
                    break;
                }
            }
            Ok::<(), Error>(())
        })
        .await;
        match search_result {
            Ok(inner) => inner?,
            Err(_) => {
                Self::stop_and_drain(engine).await;
                return Err(Error::AnalysisTimeout);
            }
        }
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
