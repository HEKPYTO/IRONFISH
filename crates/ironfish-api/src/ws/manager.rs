use super::protocol::ServerMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

pub struct SessionHandle {
    pub tx: mpsc::Sender<ServerMessage>,
    pub subscriptions: Arc<RwLock<std::collections::HashSet<String>>>,
}

pub struct SessionManager {
    sessions: RwLock<HashMap<Uuid, SessionHandle>>,
    max_connections: usize,
}

impl SessionManager {
    pub fn new(max_connections: usize) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            max_connections,
        }
    }

    pub async fn register(
        &self,
        session_id: Uuid,
        tx: mpsc::Sender<ServerMessage>,
    ) -> Result<(), &'static str> {
        let mut sessions = self.sessions.write().await;
        if sessions.len() >= self.max_connections {
            return Err("connection limit reached");
        }
        sessions.insert(
            session_id,
            SessionHandle {
                tx,
                subscriptions: Arc::new(RwLock::new(std::collections::HashSet::new())),
            },
        );
        Ok(())
    }

    pub async fn unregister(&self, session_id: &Uuid) {
        self.sessions.write().await.remove(session_id);
    }

    pub async fn broadcast_to_topic(&self, topic: &str, message: ServerMessage) {
        let sessions = self.sessions.read().await;
        for handle in sessions.values() {
            let subs = handle.subscriptions.read().await;
            if subs.contains(topic) {
                let _ = handle.tx.try_send(message.clone());
            }
        }
    }

    pub async fn update_subscriptions(
        &self,
        session_id: &Uuid,
        subscriptions: &std::collections::HashSet<String>,
    ) {
        let sessions = self.sessions.read().await;
        if let Some(handle) = sessions.get(session_id) {
            let mut subs = handle.subscriptions.write().await;
            *subs = subscriptions.clone();
        }
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}
