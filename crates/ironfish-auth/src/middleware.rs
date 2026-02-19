use crate::TokenManager;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use ironfish_core::TokenStore;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};
#[derive(Clone)]
pub struct AuthLayer<S> {
    store: Arc<S>,
    manager: Arc<TokenManager>,
    enabled: bool,
    admin_key: Option<String>,
}
impl<S> AuthLayer<S>
where
    S: TokenStore,
{
    pub fn new(store: Arc<S>, manager: Arc<TokenManager>) -> Self {
        let admin_key = std::env::var("IRONFISH_ADMIN_KEY").ok();
        Self {
            store,
            manager,
            enabled: true,
            admin_key,
        }
    }
    pub fn with_admin_key(mut self, key: impl Into<String>) -> Self {
        self.admin_key = Some(key.into());
        self
    }
    pub fn disabled(store: Arc<S>, manager: Arc<TokenManager>) -> Self {
        Self {
            store,
            manager,
            enabled: false,
            admin_key: None,
        }
    }
}
impl<S, I> Layer<I> for AuthLayer<S>
where
    S: TokenStore + Clone,
{
    type Service = AuthService<S, I>;
    fn layer(&self, inner: I) -> Self::Service {
        AuthService {
            inner,
            store: self.store.clone(),
            manager: self.manager.clone(),
            enabled: self.enabled,
            admin_key: self.admin_key.clone(),
        }
    }
}
#[derive(Clone)]
pub struct AuthService<S, I> {
    inner: I,
    store: Arc<S>,
    manager: Arc<TokenManager>,
    enabled: bool,
    admin_key: Option<String>,
}
impl<S, I> Service<Request<Body>> for AuthService<S, I>
where
    S: TokenStore + 'static,
    I: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    I::Future: Send,
{
    type Response = Response;
    type Error = I::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let path = req.uri().path();
        let method = req.method().clone();
        let is_public_path = path == "/v1/health"
            || path == "/health"
            || path == "/metrics"
            || path == "/v1/ws"
            || path == "/ws"
            || (path == "/graphql" && method == Method::GET);
        let is_admin_path = path.starts_with("/_admin");
        if !self.enabled {
            return Box::pin(self.inner.call(req));
        }
        if is_public_path {
            return Box::pin(self.inner.call(req));
        }
        if is_admin_path {
            let admin_key = self.admin_key.clone();
            let admin_header = req
                .headers()
                .get("x-admin-key")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            let mut inner = self.inner.clone();
            return Box::pin(async move {
                match (admin_key, admin_header) {
                    (Some(expected), Some(provided)) if expected == provided => {
                        inner.call(req).await
                    }
                    (Some(_), _) => Ok(unauthorized_response("invalid or missing admin key")),
                    (None, _) => Ok(forbidden_response(
                        "admin endpoints require IRONFISH_ADMIN_KEY to be set",
                    )),
                }
            });
        }
        let store = self.store.clone();
        let manager = self.manager.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let auth_header = req
                .headers()
                .get("authorization")
                .and_then(|h| h.to_str().ok());
            let token_str = match auth_header {
                Some(h) if h.starts_with("Bearer ") => &h[7..],
                _ => {
                    return Ok(unauthorized_response("missing authorization header"));
                }
            };
            if !TokenManager::validate_format(token_str) {
                return Ok(unauthorized_response("invalid token format"));
            }
            let raw_token = match TokenManager::extract_raw_token(token_str) {
                Some(r) => r,
                None => {
                    return Ok(unauthorized_response("invalid token"));
                }
            };
            let token_hash = manager.hash_token(raw_token);
            let token = match store.get_by_hash(&token_hash).await {
                Ok(Some(t)) => t,
                Ok(None) => {
                    return Ok(unauthorized_response("invalid token"));
                }
                Err(_) => {
                    return Ok(error_response("internal error"));
                }
            };
            if !token.is_valid() {
                return Ok(unauthorized_response("token expired or revoked"));
            }
            let mut updated_token = token.clone();
            updated_token.last_used_at = Some(Utc::now());
            drop(tokio::spawn(async move {
                let _ = store.update(updated_token).await;
            }));
            inner.call(req).await
        })
    }
}
fn unauthorized_response(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "application/json")],
        format!(r#"{{"error":"{}"}}"#, message),
    )
        .into_response()
}
fn error_response(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [("content-type", "application/json")],
        format!(r#"{{"error":"{}"}}"#, message),
    )
        .into_response()
}
fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        [("content-type", "application/json")],
        format!(r#"{{"error":"{}"}}"#, message),
    )
        .into_response()
}
