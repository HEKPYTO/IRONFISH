use crate::graphql::GraphQLService;
use crate::grpc::GrpcService;
use crate::rest::RestRouter;
use axum::Router;
use http_body_util::BodyExt;
use ironfish_auth::{AuthLayer, SledTokenStore, TokenManager};
use ironfish_cluster::{MembershipManager, Node};
use ironfish_core::{ApiToken, GossipMessage};
use ironfish_stockfish::AnalysisService;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::{Service, ServiceBuilder};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
pub type GossipBroadcaster = broadcast::Sender<GossipMessage>;
#[derive(Clone)]
pub struct ApiState {
    pub analysis: Arc<AnalysisService>,
    pub token_store: Arc<SledTokenStore>,
    pub token_manager: Arc<TokenManager>,
    pub node: Arc<Node>,
    pub membership: Arc<MembershipManager>,
    pub gossip_tx: Option<GossipBroadcaster>,
}
impl ApiState {
    pub fn new(
        analysis: Arc<AnalysisService>,
        token_store: Arc<SledTokenStore>,
        token_manager: Arc<TokenManager>,
        node: Arc<Node>,
        membership: Arc<MembershipManager>,
    ) -> Self {
        Self {
            analysis,
            token_store,
            token_manager,
            node,
            membership,
            gossip_tx: None,
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
    pub fn build_grpc_router(&self) -> tonic::transport::server::Router {
        let grpc_service = GrpcService::new(self.state.clone());
        let reflection_service = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
            .build_v1()
            .expect("reflection service");
        tonic::transport::Server::builder()
            .add_service(reflection_service)
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
        let grpc = self.build_grpc_router().into_service();
        tower::service_fn(move |req: axum::http::Request<axum::body::Body>| {
            let mut rest = rest.clone();
            let mut grpc = grpc.clone();
            async move {
                let is_grpc = req
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .map(|v| v.as_bytes().starts_with(b"application/grpc"))
                    .unwrap_or(false);
                if is_grpc {
                    let req = req.map(|b| {
                        b.map_err(|e| tonic::Status::unknown(e.to_string()))
                            .boxed_unsync()
                    });
                    match grpc.call(req).await {
                        Ok(res) => Ok(res.map(axum::body::Body::new)),
                        Err(_e) => Ok(axum::http::Response::builder()
                            .status(500)
                            .body(axum::body::Body::empty())
                            .unwrap()),
                    }
                } else {
                    rest.call(req).await
                }
            }
        })
    }
}
