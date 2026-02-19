use crate::graphql::GraphQLService;
use crate::grpc::GrpcService;
use crate::rest::RestRouter;
use crate::ws;
use axum::Router;
use ironfish_auth::{AuthLayer, SledTokenStore, TokenManager};
use ironfish_cluster::{MembershipManager, Node};
use ironfish_core::{ApiToken, GossipMessage};
use ironfish_stockfish::AnalysisService;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
pub type GossipBroadcaster = broadcast::Sender<GossipMessage>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebSocketConfig {
    pub enabled: bool,
    pub max_connections: usize,
    pub auth_timeout_secs: u64,
    pub ping_interval_secs: u64,
    pub max_message_size_bytes: usize,
    pub max_analyses_per_session: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_connections: 256,
            auth_timeout_secs: 5,
            ping_interval_secs: 30,
            max_message_size_bytes: 65536,
            max_analyses_per_session: 4,
        }
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub analysis: Arc<AnalysisService>,
    pub token_store: Arc<SledTokenStore>,
    pub token_manager: Arc<TokenManager>,
    pub node: Arc<Node>,
    pub membership: Arc<MembershipManager>,
    pub gossip_tx: Option<GossipBroadcaster>,
    pub ws_sessions: Arc<ws::SessionManager>,
    pub ws_config: Arc<WebSocketConfig>,
}
impl ApiState {
    pub fn new(
        analysis: Arc<AnalysisService>,
        token_store: Arc<SledTokenStore>,
        token_manager: Arc<TokenManager>,
        node: Arc<Node>,
        membership: Arc<MembershipManager>,
        ws_sessions: Arc<ws::SessionManager>,
        ws_config: WebSocketConfig,
    ) -> Self {
        Self {
            analysis,
            token_store,
            token_manager,
            node,
            membership,
            gossip_tx: None,
            ws_sessions,
            ws_config: Arc::new(ws_config),
        }
    }
    pub fn with_gossip(mut self, tx: GossipBroadcaster) -> Self {
        self.gossip_tx = Some(tx);
        self
    }
    pub fn broadcast_token_created(&self, token: ApiToken) {
        if let Some(ref tx) = self.gossip_tx {
            let _ = tx.send(GossipMessage::TokenCreated(token));
        }
    }
    pub fn broadcast_token_revoked(&self, token_id: uuid::Uuid) {
        if let Some(ref tx) = self.gossip_tx {
            let _ = tx.send(GossipMessage::TokenRevoked(token_id));
        }
    }
}
#[derive(Clone)]
pub struct ApiRouter {
    state: Arc<ApiState>,
    auth_enabled: bool,
}
impl ApiRouter {
    pub fn new(state: Arc<ApiState>) -> Self {
        Self {
            state,
            auth_enabled: true,
        }
    }
    #[allow(dead_code)]
    pub fn with_auth(mut self, enabled: bool) -> Self {
        self.auth_enabled = enabled;
        self
    }
    pub fn build_rest_router(self) -> Router {
        let rest_router = RestRouter::new(self.state.clone()).build();
        let graphql_service = GraphQLService::new(self.state.clone());
        let graphql_router = graphql_service.router();
        let app = rest_router.merge(graphql_router);
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        if self.auth_enabled {
            let auth_layer = AuthLayer::new(
                self.state.token_store.clone(),
                self.state.token_manager.clone(),
            );
            app.layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new())
                    .layer(cors)
                    .layer(auth_layer),
            )
        } else {
            app.layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new())
                    .layer(cors),
            )
        }
    }
    pub fn build_grpc_routes(&self) -> tonic::service::Routes {
        let grpc_service = GrpcService::new(self.state.clone());
        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("reflection service");
        tonic::service::Routes::new(reflection_service)
            .add_service(grpc_service.chess_server())
            .add_service(grpc_service.admin_server())
    }
    pub fn build_multiplex_service(
        self,
    ) -> impl tower::Service<
        axum::http::Request<axum::body::Body>,
        Response = axum::http::Response<axum::body::Body>,
        Error = std::convert::Infallible,
        Future = impl Send,
    > + Clone {
        let rest = self.clone().build_rest_router();
        let grpc = self.build_grpc_routes().into_axum_router();
        rest.fallback_service(grpc).into_service()
    }
}
