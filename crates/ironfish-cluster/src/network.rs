use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};
use ironfish_core::{Error, GossipMessage, NodeId, NodeInfo, Result};
const GOSSIP_PORT_OFFSET: u16 = 100;
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    Gossip(GossipEnvelope),
    SyncRequest { from_version: u64 },
    SyncResponse { entries: Vec<GossipEnvelope> },
    Ping,
    Pong,
    DiscoveryRequest,
    DiscoveryResponse { nodes: Vec<NodeInfo> },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipEnvelope {
    pub message: GossipMessage,
    pub origin: NodeId,
    pub version: u64,
    pub hops: u8,
}
pub struct NetworkService {
    local_node: NodeInfo,
    peers: Arc<RwLock<HashMap<NodeId, PeerConnection>>>,
    incoming_tx: mpsc::Sender<GossipEnvelope>,
    pub incoming_rx: Arc<RwLock<mpsc::Receiver<GossipEnvelope>>>,
    shutdown_tx: broadcast::Sender<()>,
    gossip_port: u16,
}
#[derive(Debug, Clone)]
struct PeerConnection {
    info: NodeInfo,
    gossip_addr: SocketAddr,
    healthy: bool,
    last_seen: std::time::Instant,
}
impl NetworkService {
    pub fn new(local_node: NodeInfo) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel(1024);
        let (shutdown_tx, _) = broadcast::channel(1);
        let gossip_port = local_node.address.port() + GOSSIP_PORT_OFFSET;
        Self {
            local_node,
            peers: Arc::new(RwLock::new(HashMap::new())),
            incoming_tx,
            incoming_rx: Arc::new(RwLock::new(incoming_rx)),
            shutdown_tx,
            gossip_port,
        }
    }
    pub async fn start(&self) -> Result<()> {
        let listener_addr = SocketAddr::new(self.local_node.address.ip(), self.gossip_port);
        let listener = TcpListener::bind(listener_addr).await.map_err(|e| {
            Error::Network(format!(
                "failed to bind gossip port {}: {}",
                self.gossip_port, e
            ))
        })?;
        info!("gossip network listening on {}", listener_addr);
        let incoming_tx = self.incoming_tx.clone();
        let peers = self.peers.clone();
        let local_id = self.local_node.id.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                debug!("accepted gossip connection from {}", addr);
                                let tx = incoming_tx.clone();
                                let peers_clone = peers.clone();
                                let local_id_clone = local_id.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, tx, peers_clone, local_id_clone).await {
                                        debug!("connection handler error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                warn!("accept error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("gossip network shutting down");
                        break;
                    }
                }
            }
        });
        Ok(())
    }
    pub async fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }
    pub async fn add_peer(&self, peer: NodeInfo) {
        let mut peers = self.peers.write().await;
        if peer.id == self.local_node.id {
            return;
        }
        if !peers.contains_key(&peer.id) {
            let gossip_addr =
                SocketAddr::new(peer.address.ip(), peer.address.port() + GOSSIP_PORT_OFFSET);
            peers.insert(
                peer.id.clone(),
                PeerConnection {
                    info: peer.clone(),
                    gossip_addr,
                    healthy: true,
                    last_seen: std::time::Instant::now(),
                },
            );
            info!("added peer {} at {}", peer.id, gossip_addr);
        }
    }
    pub async fn remove_peer(&self, peer_id: &NodeId) {
        let mut peers = self.peers.write().await;
        peers.remove(peer_id);
    }
    pub async fn broadcast(&self, envelope: GossipEnvelope) -> Result<()> {
        let peers = self.peers.read().await;
        for (peer_id, conn) in peers.iter() {
            if !conn.healthy {
                continue;
            }
            let envelope_clone = envelope.clone();
            let addr = conn.gossip_addr;
            let peer_id_clone = peer_id.clone();
            let peers_clone = self.peers.clone();
            tokio::spawn(async move {
                if let Err(e) = send_to_peer(addr, NetworkMessage::Gossip(envelope_clone)).await {
                    debug!("failed to send to {}: {}", peer_id_clone, e);
                    let mut peers = peers_clone.write().await;
                    if let Some(conn) = peers.get_mut(&peer_id_clone) {
                        conn.healthy = false;
                    }
                }
            });
        }
        Ok(())
    }
    pub async fn sync_with_peer(
        &self,
        peer_id: &NodeId,
        from_version: u64,
    ) -> Result<Vec<GossipEnvelope>> {
        let peers = self.peers.read().await;
        let conn = peers
            .get(peer_id)
            .ok_or_else(|| Error::Network(format!("peer {} not found", peer_id)))?;
        let response = send_and_receive(
            conn.gossip_addr,
            NetworkMessage::SyncRequest { from_version },
        )
        .await?;
        match response {
            NetworkMessage::SyncResponse { entries } => Ok(entries),
            _ => Err(Error::Network("unexpected response".into())),
        }
    }
    pub async fn discover_from_peer(&self, peer_id: &NodeId) -> Result<Vec<NodeInfo>> {
        let peers = self.peers.read().await;
        let conn = peers
            .get(peer_id)
            .ok_or_else(|| Error::Network(format!("peer {} not found", peer_id)))?;
        let response = send_and_receive(conn.gossip_addr, NetworkMessage::DiscoveryRequest).await?;
        match response {
            NetworkMessage::DiscoveryResponse { nodes } => Ok(nodes),
            _ => Err(Error::Network("unexpected response".into())),
        }
    }
    pub async fn receive(&self) -> Option<GossipEnvelope> {
        let mut rx = self.incoming_rx.write().await;
        rx.recv().await
    }
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
    pub async fn healthy_peers(&self) -> Vec<NodeInfo> {
        self.peers
            .read()
            .await
            .values()
            .filter(|c| c.healthy)
            .map(|c| c.info.clone())
            .collect()
    }
    pub async fn mark_healthy(&self, peer_id: &NodeId) {
        let mut peers = self.peers.write().await;
        if let Some(conn) = peers.get_mut(peer_id) {
            conn.healthy = true;
            conn.last_seen = std::time::Instant::now();
        }
    }
}
async fn handle_connection(
    mut stream: TcpStream,
    incoming_tx: mpsc::Sender<GossipEnvelope>,
    peers: Arc<RwLock<HashMap<NodeId, PeerConnection>>>,
    _local_id: NodeId,
) -> Result<()> {
    loop {
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(Error::Network(format!("read error: {}", e))),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(Error::Network("message too large".into()));
        }
        let mut buf = vec![0u8; len];
        stream
            .read_exact(&mut buf)
            .await
            .map_err(|e| Error::Network(format!("read error: {}", e)))?;
        let message: NetworkMessage = serde_json::from_slice(&buf)
            .map_err(|e| Error::Network(format!("deserialize error: {}", e)))?;
        match message {
            NetworkMessage::Gossip(envelope) => {
                if let Err(e) = incoming_tx.send(envelope).await {
                    error!("failed to queue incoming message: {}", e);
                }
            }
            NetworkMessage::Ping => {
                let response = NetworkMessage::Pong;
                send_response(&mut stream, &response).await?;
            }
            NetworkMessage::SyncRequest { from_version: _ } => {
                let response = NetworkMessage::SyncResponse { entries: vec![] };
                send_response(&mut stream, &response).await?;
            }
            NetworkMessage::DiscoveryRequest => {
                let peers_guard = peers.read().await;
                let nodes: Vec<NodeInfo> = peers_guard.values().map(|c| c.info.clone()).collect();
                let response = NetworkMessage::DiscoveryResponse { nodes };
                send_response(&mut stream, &response).await?;
            }
            _ => {}
        }
    }
    Ok(())
}
async fn send_response(stream: &mut TcpStream, message: &NetworkMessage) -> Result<()> {
    let data = serde_json::to_vec(message)
        .map_err(|e| Error::Network(format!("serialize error: {}", e)))?;
    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    stream
        .write_all(&data)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    Ok(())
}
async fn send_to_peer(addr: SocketAddr, message: NetworkMessage) -> Result<()> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| Error::Network(format!("connect error: {}", e)))?;
    let data = serde_json::to_vec(&message)
        .map_err(|e| Error::Network(format!("serialize error: {}", e)))?;
    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    stream
        .write_all(&data)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    Ok(())
}
async fn send_and_receive(addr: SocketAddr, message: NetworkMessage) -> Result<NetworkMessage> {
    let mut stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .map_err(|_| Error::Network("connection timeout".into()))?
        .map_err(|e| Error::Network(format!("connect error: {}", e)))?;
    let data = serde_json::to_vec(&message)
        .map_err(|e| Error::Network(format!("serialize error: {}", e)))?;
    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    stream
        .write_all(&data)
        .await
        .map_err(|e| Error::Network(format!("write error: {}", e)))?;
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len_buf))
        .await
        .map_err(|_| Error::Network("read timeout".into()))?
        .map_err(|e| Error::Network(format!("read error: {}", e)))?;
    let response_len = u32::from_be_bytes(len_buf) as usize;
    if response_len > MAX_MESSAGE_SIZE {
        return Err(Error::Network("response too large".into()));
    }
    let mut buf = vec![0u8; response_len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| Error::Network(format!("read error: {}", e)))?;
    serde_json::from_slice(&buf).map_err(|e| Error::Network(format!("deserialize error: {}", e)))
}
