use std::sync::Arc;

use async_graphql::{Context, InputObject, Object, SimpleObject};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use ironfish_core::{AnalysisRequest, BestMoveRequest, CreateTokenRequest, TokenStore};

use crate::ApiState;

#[derive(SimpleObject)]
pub struct Move {
    pub from: String,
    pub to: String,
    pub promotion: Option<String>,
}

#[derive(SimpleObject)]
pub struct Evaluation {
    pub score_type: String,
    pub value: i32,
}

#[derive(SimpleObject)]
pub struct PrincipalVariation {
    pub rank: u32,
    pub moves: Vec<Move>,
    pub evaluation: Evaluation,
    pub depth: u32,
}

#[derive(SimpleObject)]
pub struct Analysis {
    pub id: String,
    pub fen: String,
    pub best_move: Move,
    pub ponder: Option<Move>,
    pub evaluation: Evaluation,
    pub principal_variations: Vec<PrincipalVariation>,
    pub depth_reached: u32,
    pub nodes_searched: u64,
    pub time_ms: u64,
}

#[derive(SimpleObject)]
pub struct BestMoveResult {
    pub best_move: Move,
    pub ponder: Option<Move>,
}

#[derive(SimpleObject)]
pub struct NodeStatus {
    pub id: String,
    pub address: String,
    pub state: String,
    pub uptime_seconds: u64,
}

#[derive(SimpleObject)]
pub struct ClusterStatus {
    pub nodes: Vec<NodeStatus>,
    pub leader_id: Option<String>,
    pub term: u64,
    pub healthy: bool,
}

#[derive(SimpleObject)]
pub struct Token {
    pub id: String,
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(SimpleObject)]
pub struct TokenInfo {
    pub id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

#[derive(Default)]
pub struct AnalysisQuery;

#[Object]
impl AnalysisQuery {
    async fn analyze(
        &self,
        ctx: &Context<'_>,
        fen: String,
        depth: Option<u32>,
        multipv: Option<u32>,
    ) -> async_graphql::Result<Analysis> {
        let state = ctx.data::<Arc<ApiState>>()?;

        let request = AnalysisRequest::new(&fen)
            .with_depth(depth.unwrap_or(20) as u8)
            .with_multipv(multipv.unwrap_or(1) as u8);

        let result = state.analysis.analyze(request).await?;

        Ok(Analysis {
            id: result.id.to_string(),
            fen: result.fen,
            best_move: Move {
                from: result.best_move.from,
                to: result.best_move.to,
                promotion: result.best_move.promotion.map(|c| c.to_string()),
            },
            ponder: result.ponder.map(|m| Move {
                from: m.from,
                to: m.to,
                promotion: m.promotion.map(|c| c.to_string()),
            }),
            evaluation: Evaluation {
                score_type: format!("{:?}", result.evaluation.score_type),
                value: result.evaluation.value,
            },
            principal_variations: result
                .principal_variations
                .into_iter()
                .map(|pv| PrincipalVariation {
                    rank: pv.rank as u32,
                    moves: pv
                        .moves
                        .into_iter()
                        .map(|m| Move {
                            from: m.from,
                            to: m.to,
                            promotion: m.promotion.map(|c| c.to_string()),
                        })
                        .collect(),
                    evaluation: Evaluation {
                        score_type: format!("{:?}", pv.evaluation.score_type),
                        value: pv.evaluation.value,
                    },
                    depth: pv.depth as u32,
                })
                .collect(),
            depth_reached: result.depth_reached as u32,
            nodes_searched: result.nodes_searched,
            time_ms: result.time_ms,
        })
    }

    async fn best_move(
        &self,
        ctx: &Context<'_>,
        fen: String,
        movetime: Option<u64>,
    ) -> async_graphql::Result<BestMoveResult> {
        let state = ctx.data::<Arc<ApiState>>()?;

        let request = BestMoveRequest { fen, movetime };

        let result = state.analysis.best_move(request).await?;

        Ok(BestMoveResult {
            best_move: Move {
                from: result.best_move.from,
                to: result.best_move.to,
                promotion: result.best_move.promotion.map(|c| c.to_string()),
            },
            ponder: result.ponder.map(|m| Move {
                from: m.from,
                to: m.to,
                promotion: m.promotion.map(|c| c.to_string()),
            }),
        })
    }
}

#[derive(Default)]
pub struct AnalysisMutation;

#[Object]
impl AnalysisMutation {
    async fn start_analysis(
        &self,
        ctx: &Context<'_>,
        fen: String,
        depth: Option<u32>,
    ) -> async_graphql::Result<String> {
        let state = ctx.data::<Arc<ApiState>>()?;

        let request = AnalysisRequest::new(&fen).with_depth(depth.unwrap_or(20) as u8);

        let result = state.analysis.analyze(request).await?;
        Ok(result.id.to_string())
    }
}

#[derive(Default)]
pub struct ClusterQuery;

#[Object]
impl ClusterQuery {
    async fn cluster_status(&self, ctx: &Context<'_>) -> async_graphql::Result<ClusterStatus> {
        let state = ctx.data::<Arc<ApiState>>()?;
        let status = state.membership.cluster_status().await;

        Ok(ClusterStatus {
            nodes: status
                .nodes
                .into_iter()
                .map(|n| NodeStatus {
                    id: n.info.id.to_string(),
                    address: n.info.address.to_string(),
                    state: format!("{:?}", n.state),
                    uptime_seconds: n.uptime_seconds,
                })
                .collect(),
            leader_id: status.leader.map(|l| l.to_string()),
            term: status.term,
            healthy: status.healthy,
        })
    }
}

#[derive(Default)]
pub struct TokenQuery;

#[Object]
impl TokenQuery {
    async fn tokens(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<TokenInfo>> {
        let state = ctx.data::<Arc<ApiState>>()?;
        let tokens = state.token_store.list().await?;

        Ok(tokens
            .into_iter()
            .map(|t| TokenInfo {
                id: t.id.to_string(),
                name: t.name,
                created_at: t.created_at,
                expires_at: t.expires_at,
                revoked: t.revoked,
            })
            .collect())
    }
}

#[derive(Default)]
pub struct TokenMutation;

#[derive(InputObject)]
pub struct CreateTokenInput {
    pub name: Option<String>,
    pub expires_in_days: Option<u32>,
    pub rate_limit: Option<u32>,
}

#[Object]
impl TokenMutation {
    async fn create_token(
        &self,
        ctx: &Context<'_>,
        input: Option<CreateTokenInput>,
    ) -> async_graphql::Result<Token> {
        let state = ctx.data::<Arc<ApiState>>()?;

        let request = CreateTokenRequest {
            name: input.as_ref().and_then(|i| i.name.clone()),
            expires_in_days: input.as_ref().and_then(|i| i.expires_in_days),
            rate_limit: input.as_ref().and_then(|i| i.rate_limit),
        };

        let (token, response) = state.token_manager.create(request)?;
        state.token_store.create(token).await?;

        Ok(Token {
            id: response.id.to_string(),
            token: response.token,
            expires_at: response.expires_at,
        })
    }

    async fn revoke_token(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<bool> {
        let state = ctx.data::<Arc<ApiState>>()?;
        let uuid = Uuid::parse_str(&id)?;
        state.token_store.revoke(&uuid).await?;
        Ok(true)
    }
}
