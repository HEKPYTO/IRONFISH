use super::protocol::{ClientMessage, ServerMessage};
use crate::ApiState;
use ironfish_core::{AnalysisRequest, BestMoveRequest, TokenStore};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct WsSession {
    pub session_id: Uuid,
    pub authenticated: bool,
    pub tx: mpsc::Sender<ServerMessage>,
    pub active_analyses: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
    pub subscriptions: HashSet<String>,
    state: Arc<ApiState>,
    max_analyses: usize,
}

impl WsSession {
    pub fn new(
        session_id: Uuid,
        tx: mpsc::Sender<ServerMessage>,
        state: Arc<ApiState>,
        max_analyses: usize,
    ) -> Self {
        Self {
            session_id,
            authenticated: false,
            tx,
            active_analyses: Arc::new(Mutex::new(HashMap::new())),
            subscriptions: HashSet::new(),
            state,
            max_analyses,
        }
    }

    pub async fn handle_message(&mut self, msg: ClientMessage) {
        match msg {
            ClientMessage::Auth { id, token } => self.handle_auth(id, token).await,
            ClientMessage::Ping { id } => {
                let _ = self.tx.send(ServerMessage::Pong { id }).await;
            }
            ref m if !self.authenticated => {
                let id = extract_id(m);
                let _ = self
                    .tx
                    .send(ServerMessage::Error {
                        id,
                        code: 401,
                        message: "not authenticated".to_string(),
                    })
                    .await;
            }
            ClientMessage::Analyze {
                id,
                fen,
                depth,
                multipv,
                movetime,
            } => {
                self.handle_analyze(id, fen, depth, multipv, movetime).await;
            }
            ClientMessage::Cancel { id, analysis_id } => {
                self.handle_cancel(id, analysis_id).await;
            }
            ClientMessage::Bestmove { id, fen, movetime } => {
                self.handle_bestmove(id, fen, movetime).await;
            }
            ClientMessage::Subscribe { id, topics } => {
                self.handle_subscribe(id, topics).await;
            }
            ClientMessage::Unsubscribe { id, topics } => {
                self.handle_unsubscribe(id, topics).await;
            }
        }
    }

    async fn handle_auth(&mut self, id: String, token: String) {
        let raw = token.strip_prefix("iff_").unwrap_or(&token);
        let hash = self.state.token_manager.hash_token(raw);
        match self.state.token_store.get_by_hash(&hash).await {
            Ok(Some(api_token)) if api_token.is_valid() => {
                self.authenticated = true;
                let _ = self
                    .tx
                    .send(ServerMessage::AuthResult {
                        id,
                        success: true,
                        error: None,
                    })
                    .await;
            }
            _ => {
                let _ = self
                    .tx
                    .send(ServerMessage::AuthResult {
                        id,
                        success: false,
                        error: Some("invalid or expired token".to_string()),
                    })
                    .await;
            }
        }
    }

    async fn handle_analyze(
        &mut self,
        id: String,
        fen: String,
        depth: u8,
        multipv: u8,
        movetime: Option<u64>,
    ) {
        {
            let analyses = self.active_analyses.lock().await;
            if analyses.len() >= self.max_analyses {
                let _ = self
                    .tx
                    .send(ServerMessage::Error {
                        id: Some(id),
                        code: 429,
                        message: "too many concurrent analyses".to_string(),
                    })
                    .await;
                return;
            }
        }

        let mut request = AnalysisRequest::new(fen)
            .with_depth(depth)
            .with_multipv(multipv);
        if let Some(mt) = movetime {
            request = request.with_movetime(mt);
        }
        let analysis_id = request.id;
        let cancel = CancellationToken::new();
        self.active_analyses
            .lock()
            .await
            .insert(analysis_id, cancel.clone());

        let tx = self.tx.clone();
        let analysis = self.state.analysis.clone();
        let active_analyses = self.active_analyses.clone();
        tokio::spawn(async move {
            let (progress_tx, mut progress_rx) =
                mpsc::channel::<ironfish_core::AnalysisProgress>(32);

            let tx_progress = tx.clone();
            let progress_task = tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    let _ = tx_progress
                        .send(ServerMessage::AnalysisProgress {
                            analysis_id: progress.id,
                            current_depth: progress.current_depth,
                            target_depth: progress.target_depth,
                            evaluation: progress.evaluation,
                            principal_variations: progress.principal_variations,
                            nodes_per_second: progress.nodes_per_second,
                        })
                        .await;
                }
            });

            let result = analysis
                .analyze_streaming(request, progress_tx, cancel)
                .await;
            let _ = progress_task.await;

            active_analyses.lock().await.remove(&analysis_id);

            match result {
                Ok(analysis_result) => {
                    let _ = tx
                        .send(ServerMessage::AnalysisComplete {
                            id,
                            result: analysis_result,
                        })
                        .await;
                }
                Err(ironfish_core::Error::AnalysisCancelled) => {
                    let _ = tx
                        .send(ServerMessage::AnalysisCancelled { analysis_id })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerMessage::Error {
                            id: Some(id),
                            code: 500,
                            message: e.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    async fn handle_cancel(&mut self, _id: String, analysis_id: Uuid) {
        if let Some(cancel) = self.active_analyses.lock().await.remove(&analysis_id) {
            cancel.cancel();
        }
    }

    async fn handle_bestmove(&mut self, id: String, fen: String, movetime: Option<u64>) {
        let mut request = BestMoveRequest::new(fen);
        if let Some(mt) = movetime {
            request.movetime = Some(mt);
        }
        let tx = self.tx.clone();
        let analysis = self.state.analysis.clone();
        tokio::spawn(async move {
            match analysis.best_move(request).await {
                Ok(result) => {
                    let _ = tx.send(ServerMessage::BestmoveResult { id, result }).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerMessage::Error {
                            id: Some(id),
                            code: 500,
                            message: e.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    async fn handle_subscribe(&mut self, id: String, topics: Vec<String>) {
        for topic in &topics {
            self.subscriptions.insert(topic.clone());
        }
        let _ = self.tx.send(ServerMessage::Subscribed { id, topics }).await;
    }

    async fn handle_unsubscribe(&mut self, _id: String, topics: Vec<String>) {
        for topic in &topics {
            self.subscriptions.remove(topic);
        }
    }

    pub async fn cancel_all(&mut self) {
        for (_, cancel) in self.active_analyses.lock().await.drain() {
            cancel.cancel();
        }
    }
}

fn extract_id(msg: &ClientMessage) -> Option<String> {
    match msg {
        ClientMessage::Auth { id, .. }
        | ClientMessage::Analyze { id, .. }
        | ClientMessage::Cancel { id, .. }
        | ClientMessage::Bestmove { id, .. }
        | ClientMessage::Subscribe { id, .. }
        | ClientMessage::Unsubscribe { id, .. }
        | ClientMessage::Ping { id } => Some(id.clone()),
    }
}
