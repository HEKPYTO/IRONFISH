use std::sync::Arc;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use ironfish_core::{
    AnalysisRequest, AnalysisResult, BestMoveRequest, BestMoveResponse, ClusterStatus,
    CreateTokenRequest, CreateTokenResponse, JoinRequest, NodeInfo, TokenMetadata, TokenStore,
};
use crate::ApiState;
#[derive(Debug, Deserialize)]
pub struct AnalyzeBody {
    pub fen: String,
    #[serde(default = "default_depth")]
    pub depth: u8,
    #[serde(default = "default_multipv")]
    pub multipv: u8,
    pub movetime: Option<u64>,
}
fn default_depth() -> u8 {
    20
}
fn default_multipv() -> u8 {
    1
}
#[derive(Debug, Deserialize)]
pub struct BestMoveBody {
    pub fen: String,
    pub movetime: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub node_id: String,
    pub version: String,
}
#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub active_analyses: u32,
    pub queue_depth: u32,
    pub engines_available: u32,
    pub engines_total: u32,
}
pub async fn analyze(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<AnalyzeBody>,
) -> Result<Json<AnalysisResult>, (StatusCode, Json<ErrorResponse>)> {
    let request = AnalysisRequest::new(&body.fen)
        .with_depth(body.depth)
        .with_multipv(body.multipv);
    let request = match body.movetime {
        Some(ms) => request.with_movetime(ms),
        None => request,
    };
    match state.analysis.analyze(request).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
pub async fn get_analysis(
    State(_state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<Json<AnalysisResult>, (StatusCode, Json<ErrorResponse>)> {
    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("analysis {} not found", id),
        }),
    ))
}
pub async fn best_move(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<BestMoveBody>,
) -> Result<Json<BestMoveResponse>, (StatusCode, Json<ErrorResponse>)> {
    let request = BestMoveRequest {
        fen: body.fen,
        movetime: body.movetime,
    };
    match state.analysis.best_move(request).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
pub async fn health(State(state): State<Arc<ApiState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        node_id: state.node.id().to_string(),
        version: state.node.info().version.clone(),
    })
}
pub async fn health_simple() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "healthy"}))
}
pub async fn metrics(State(state): State<Arc<ApiState>>) -> Json<MetricsResponse> {
    let (active, available, total) = state
        .analysis
        .pool()
        .map(|p| (p.active() as u32, p.available() as u32, p.size() as u32))
        .unwrap_or((0, 0, 0));
    Json(MetricsResponse {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        active_analyses: active,
        queue_depth: 0,
        engines_available: available,
        engines_total: total,
    })
}
pub async fn metrics_simple() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}
pub async fn cluster_status(State(state): State<Arc<ApiState>>) -> Json<ClusterStatus> {
    let status = state.membership.cluster_status().await;
    Json(status)
}
#[derive(Debug, Deserialize)]
pub struct JoinBody {
    pub address: String,
    pub priority: Option<u32>,
}
pub async fn cluster_join(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<JoinBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let addr = body.address.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid address".to_string(),
            }),
        )
    })?;
    let node_info = NodeInfo {
        id: ironfish_core::NodeId::generate(),
        address: addr,
        priority: body.priority.unwrap_or(100),
        started_at: chrono::Utc::now(),
        version: "unknown".to_string(),
    };
    let request = JoinRequest { node_info };
    match state.membership.join(request).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
pub async fn cluster_leave(
    State(state): State<Arc<ApiState>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    match state.membership.leave(state.node.id()).await {
        Ok(_) => Ok(Json(serde_json::json!({"success": true}))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
pub async fn list_tokens(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<Vec<TokenMetadata>>, (StatusCode, Json<ErrorResponse>)> {
    match state.token_store.list().await {
        Ok(tokens) => {
            let metadata: Vec<TokenMetadata> = tokens.iter().map(TokenMetadata::from).collect();
            Ok(Json(metadata))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
#[derive(Debug, Deserialize)]
pub struct CreateTokenBody {
    pub name: Option<String>,
    pub expires_in_days: Option<u32>,
    pub rate_limit: Option<u32>,
}
pub async fn create_token(
    State(state): State<Arc<ApiState>>,
    Json(body): Json<CreateTokenBody>,
) -> Result<Json<CreateTokenResponse>, (StatusCode, Json<ErrorResponse>)> {
    let request = CreateTokenRequest {
        name: body.name,
        expires_in_days: body.expires_in_days,
        rate_limit: body.rate_limit,
    };
    match state.token_manager.create(request) {
        Ok((token, response)) => match state.token_store.create(token.clone()).await {
            Ok(_) => {
                state.broadcast_token_created(token);
                Ok(Json(response))
            }
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )),
        },
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
pub async fn revoke_token(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let uuid = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid token id".to_string(),
            }),
        )
    })?;
    match state.token_store.revoke(&uuid).await {
        Ok(_) => {
            state.broadcast_token_revoked(uuid);
            Ok(Json(serde_json::json!({"success": true})))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
