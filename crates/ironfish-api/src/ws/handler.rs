use super::protocol::{ClientMessage, ServerMessage};
use super::session::WsSession;
use crate::ApiState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use ironfish_core::TokenStore;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct WsParams {
    pub token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ApiState>>,
    Query(params): Query<WsParams>,
) -> impl IntoResponse {
    let pre_authenticated = if let Some(ref token) = params.token {
        validate_token(token, &state).await
    } else {
        false
    };

    ws.max_message_size(state.ws_config.max_message_size_bytes)
        .on_upgrade(move |socket| handle_socket(socket, state, pre_authenticated))
}

async fn validate_token(token: &str, state: &ApiState) -> bool {
    let raw = token.strip_prefix("iff_").unwrap_or(token);
    let hash = state.token_manager.hash_token(raw);
    matches!(
        state.token_store.get_by_hash(&hash).await,
        Ok(Some(ref api_token)) if api_token.is_valid()
    )
}

async fn handle_socket(socket: WebSocket, state: Arc<ApiState>, pre_authenticated: bool) {
    let session_id = Uuid::new_v4();
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(64);

    if state
        .ws_sessions
        .register(session_id, tx.clone())
        .await
        .is_err()
    {
        return;
    }

    let mut session = WsSession::new(
        session_id,
        tx,
        state.clone(),
        state.ws_config.max_analyses_per_session,
    );
    if pre_authenticated {
        session.authenticated = true;
    }

    let auth_timeout = Duration::from_secs(state.ws_config.auth_timeout_secs);
    let ping_interval_duration = Duration::from_secs(state.ws_config.ping_interval_secs);

    use axum::extract::ws::Message as WsMsg;
    use futures::SinkExt;

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_sender.send(WsMsg::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut ping_interval = interval(ping_interval_duration);
    ping_interval.tick().await;

    if !session.authenticated {
        let auth_deadline = tokio::time::sleep(auth_timeout);
        tokio::pin!(auth_deadline);

        loop {
            tokio::select! {
                _ = &mut auth_deadline => {
                    let _ = session.tx.send(ServerMessage::Error {
                        id: None,
                        code: 401,
                        message: "auth timeout".to_string(),
                    }).await;
                    break;
                }
                msg = ws_receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<ClientMessage>(&text) {
                                Ok(client_msg) => {
                                    session.handle_message(client_msg).await;
                                    if session.authenticated {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    let _ = session.tx.send(ServerMessage::Error {
                                        id: None,
                                        code: 400,
                                        message: format!("invalid message: {}", e),
                                    }).await;
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            cleanup(&state, &mut session, session_id).await;
                            writer_task.abort();
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }

        if !session.authenticated {
            cleanup(&state, &mut session, session_id).await;
            writer_task.abort();
            return;
        }
    }

    use futures::StreamExt;
    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                let _ = session.tx.send(ServerMessage::Pong {
                    id: "server-ping".to_string(),
                }).await;
            }
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(client_msg) => {
                                session.handle_message(client_msg).await;
                                state.ws_sessions.update_subscriptions(
                                    &session_id,
                                    &session.subscriptions,
                                ).await;
                            }
                            Err(e) => {
                                let _ = session.tx.send(ServerMessage::Error {
                                    id: None,
                                    code: 400,
                                    message: format!("invalid message: {}", e),
                                }).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(_))) => {
                        debug!(session_id = %session_id, "received ws ping");
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    cleanup(&state, &mut session, session_id).await;
    writer_task.abort();
}

async fn cleanup(state: &ApiState, session: &mut WsSession, session_id: Uuid) {
    debug!(session_id = %session_id, "ws session disconnected");
    session.cancel_all().await;
    state.ws_sessions.unregister(&session_id).await;
}
