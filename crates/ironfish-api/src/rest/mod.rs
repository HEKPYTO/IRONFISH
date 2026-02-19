mod handlers;
use crate::ws;
use crate::ApiState;
use axum::routing::{delete, get, post};
use axum::Router;
pub use handlers::*;
use std::sync::Arc;
pub struct RestRouter {
    state: Arc<ApiState>,
}
impl RestRouter {
    pub fn new(state: Arc<ApiState>) -> Self {
        Self { state }
    }
    pub fn build(self) -> Router {
        let api_routes = Router::new()
            .route("/analyze", post(handlers::analyze))
            .route("/analyze/{id}", get(handlers::get_analysis))
            .route("/bestmove", post(handlers::best_move))
            .route("/health", get(handlers::health))
            .route("/metrics", get(handlers::metrics))
            .route("/ws", get(ws::ws_handler))
            .with_state(self.state.clone());
        let admin_routes = Router::new()
            .route("/cluster/status", get(handlers::cluster_status))
            .route("/cluster/join", post(handlers::cluster_join))
            .route("/cluster/leave", post(handlers::cluster_leave))
            .route(
                "/tokens",
                get(handlers::list_tokens).post(handlers::create_token),
            )
            .route("/tokens/{id}", delete(handlers::revoke_token))
            .with_state(self.state.clone());
        Router::new()
            .nest("/v1", api_routes)
            .nest("/_admin", admin_routes)
            .route("/health", get(handlers::health_simple))
            .route("/metrics", get(handlers::metrics_simple))
    }
}
