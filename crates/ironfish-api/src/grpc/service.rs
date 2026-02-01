use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use tonic::{Request, Response, Status};

use ironfish_core::{AnalysisRequest, BestMoveRequest};

use crate::proto::{
    chess_analysis_server::{ChessAnalysis, ChessAnalysisServer},
    cluster_admin_server::{ClusterAdmin, ClusterAdminServer},
    AnalysisUpdate, AnalyzeRequest as ProtoAnalyzeRequest, AnalyzeResponse as ProtoAnalyzeResponse,
    BestMoveRequest as ProtoBestMoveRequest, BestMoveResponse as ProtoBestMoveResponse,
    ClusterStatus as ProtoClusterStatus, Empty, Evaluation as ProtoEvaluation,
    JoinRequest as ProtoJoinRequest, JoinResponse as ProtoJoinResponse,
    LeaveRequest as ProtoLeaveRequest, LeaveResponse as ProtoLeaveResponse, Move as ProtoMove,
    NodeStatus as ProtoNodeStatus, PrincipalVariation as ProtoPv, ScoreType as ProtoScoreType,
};
use crate::ApiState;

pub struct GrpcService {
    state: Arc<ApiState>,
}

impl GrpcService {
    pub fn new(state: Arc<ApiState>) -> Self {
        Self { state }
    }

    pub fn chess_server(&self) -> ChessAnalysisServer<ChessAnalysisHandler> {
        ChessAnalysisServer::new(ChessAnalysisHandler {
            state: self.state.clone(),
        })
    }

    pub fn admin_server(&self) -> ClusterAdminServer<ClusterAdminHandler> {
        ClusterAdminServer::new(ClusterAdminHandler {
            state: self.state.clone(),
        })
    }
}

pub struct ChessAnalysisHandler {
    state: Arc<ApiState>,
}

#[tonic::async_trait]
impl ChessAnalysis for ChessAnalysisHandler {
    async fn analyze(
        &self,
        request: Request<ProtoAnalyzeRequest>,
    ) -> Result<Response<ProtoAnalyzeResponse>, Status> {
        let req = request.into_inner();

        let analysis_req = AnalysisRequest::new(&req.fen)
            .with_depth(req.depth as u8)
            .with_multipv(req.multipv as u8);

        let analysis_req = match req.movetime_ms {
            Some(ms) => analysis_req.with_movetime(ms),
            None => analysis_req,
        };

        let result = self
            .state
            .analysis
            .analyze(analysis_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let best_move = ProtoMove {
            from: result.best_move.from.clone(),
            to: result.best_move.to.clone(),
            promotion: result.best_move.promotion.map(|c| c.to_string()),
        };

        let ponder = result.ponder.map(|m| ProtoMove {
            from: m.from,
            to: m.to,
            promotion: m.promotion.map(|c| c.to_string()),
        });

        let evaluation = ProtoEvaluation {
            score_type: match result.evaluation.score_type {
                ironfish_core::ScoreType::Centipawns => ProtoScoreType::Centipawns as i32,
                ironfish_core::ScoreType::Mate => ProtoScoreType::Mate as i32,
            },
            value: result.evaluation.value,
        };

        let pvs: Vec<ProtoPv> = result
            .principal_variations
            .into_iter()
            .map(|pv| ProtoPv {
                rank: pv.rank as u32,
                moves: pv
                    .moves
                    .into_iter()
                    .map(|m| ProtoMove {
                        from: m.from,
                        to: m.to,
                        promotion: m.promotion.map(|c| c.to_string()),
                    })
                    .collect(),
                evaluation: Some(ProtoEvaluation {
                    score_type: match pv.evaluation.score_type {
                        ironfish_core::ScoreType::Centipawns => ProtoScoreType::Centipawns as i32,
                        ironfish_core::ScoreType::Mate => ProtoScoreType::Mate as i32,
                    },
                    value: pv.evaluation.value,
                }),
                depth: pv.depth as u32,
            })
            .collect();

        Ok(Response::new(ProtoAnalyzeResponse {
            id: result.id.to_string(),
            fen: result.fen,
            best_move: Some(best_move),
            ponder,
            evaluation: Some(evaluation),
            principal_variations: pvs,
            depth_reached: result.depth_reached as u32,
            nodes_searched: result.nodes_searched,
            time_ms: result.time_ms,
        }))
    }

    async fn best_move(
        &self,
        request: Request<ProtoBestMoveRequest>,
    ) -> Result<Response<ProtoBestMoveResponse>, Status> {
        let req = request.into_inner();

        let best_move_req = BestMoveRequest {
            fen: req.fen,
            movetime: req.movetime_ms,
        };

        let result = self
            .state
            .analysis
            .best_move(best_move_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let best_move = ProtoMove {
            from: result.best_move.from,
            to: result.best_move.to,
            promotion: result.best_move.promotion.map(|c| c.to_string()),
        };

        let ponder = result.ponder.map(|m| ProtoMove {
            from: m.from,
            to: m.to,
            promotion: m.promotion.map(|c| c.to_string()),
        });

        Ok(Response::new(ProtoBestMoveResponse {
            best_move: Some(best_move),
            ponder,
        }))
    }

    type StreamAnalysisStream =
        Pin<Box<dyn Stream<Item = Result<AnalysisUpdate, Status>> + Send>>;

    async fn stream_analysis(
        &self,
        request: Request<ProtoAnalyzeRequest>,
    ) -> Result<Response<Self::StreamAnalysisStream>, Status> {
        let req = request.into_inner();

        let stream = async_stream::stream! {
            yield Ok(AnalysisUpdate {
                id: uuid::Uuid::new_v4().to_string(),
                current_depth: 1,
                target_depth: req.depth,
                current_move: None,
                nodes_per_second: 0,
            });
        };

        Ok(Response::new(Box::pin(stream)))
    }
}

pub struct ClusterAdminHandler {
    state: Arc<ApiState>,
}

#[tonic::async_trait]
impl ClusterAdmin for ClusterAdminHandler {
    async fn get_status(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<ProtoClusterStatus>, Status> {
        let status = self.state.membership.cluster_status().await;

        let nodes: Vec<ProtoNodeStatus> = status
            .nodes
            .into_iter()
            .map(|n| ProtoNodeStatus {
                id: n.info.id.to_string(),
                address: n.info.address.to_string(),
                state: format!("{:?}", n.state),
                uptime_seconds: n.uptime_seconds,
            })
            .collect();

        Ok(Response::new(ProtoClusterStatus {
            nodes,
            leader_id: status.leader.map(|l| l.to_string()),
            term: status.term,
            healthy: status.healthy,
        }))
    }

    async fn join_cluster(
        &self,
        request: Request<ProtoJoinRequest>,
    ) -> Result<Response<ProtoJoinResponse>, Status> {
        let req = request.into_inner();

        let addr = req
            .address
            .parse()
            .map_err(|_| Status::invalid_argument("invalid address"))?;

        let node_info = ironfish_core::NodeInfo {
            id: ironfish_core::NodeId::from_string(req.node_id),
            address: addr,
            priority: req.priority,
            started_at: chrono::Utc::now(),
            version: "unknown".to_string(),
        };

        let join_req = ironfish_core::JoinRequest { node_info };

        let result = self
            .state
            .membership
            .join(join_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ProtoJoinResponse {
            accepted: result.accepted,
            leader_id: result.leader_id.map(|l| l.to_string()),
            members: result.members.into_iter().map(|m| m.id.to_string()).collect(),
            term: result.term,
        }))
    }

    async fn leave_cluster(
        &self,
        request: Request<ProtoLeaveRequest>,
    ) -> Result<Response<ProtoLeaveResponse>, Status> {
        let req = request.into_inner();
        let node_id = ironfish_core::NodeId::from_string(req.node_id);

        self.state
            .membership
            .leave(&node_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ProtoLeaveResponse { success: true }))
    }
}
