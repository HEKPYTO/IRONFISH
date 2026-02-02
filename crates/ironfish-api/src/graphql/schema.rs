use std::sync::Arc;
use async_graphql::{EmptySubscription, MergedObject, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::routing::post;
use axum::Router;
use crate::ApiState;
use super::resolvers::{AnalysisMutation, AnalysisQuery, ClusterQuery, TokenMutation, TokenQuery};
#[derive(MergedObject, Default)]
pub struct QueryRoot(AnalysisQuery, ClusterQuery, TokenQuery);
#[derive(MergedObject, Default)]
pub struct MutationRoot(AnalysisMutation, TokenMutation);
pub type AppSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;
pub struct GraphQLService {
    state: Arc<ApiState>,
}
impl GraphQLService {
    pub fn new(state: Arc<ApiState>) -> Self {
        Self { state }
    }
    pub fn schema(&self) -> AppSchema {
        Schema::build(
            QueryRoot::default(),
            MutationRoot::default(),
            EmptySubscription,
        )
        .data(self.state.clone())
        .finish()
    }
    pub fn router(self) -> Router {
        let schema = self.schema();
        Router::new()
            .route("/graphql", post(graphql_handler).get(graphql_playground))
            .with_state(schema)
    }
}
async fn graphql_handler(State(schema): State<AppSchema>, req: GraphQLRequest) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}
async fn graphql_playground() -> impl axum::response::IntoResponse {
    axum::response::Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}
