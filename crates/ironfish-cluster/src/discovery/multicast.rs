use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use ironfish_core::{ClusterDiscovery, Error, NodeId, NodeInfo, Result};

const DISCOVERY_MSG_ANNOUNCE: u8 = 1;
const DISCOVERY_MSG_WITHDRAW: u8 = 2;

pub struct MulticastDiscovery {
    group: Ipv4Addr,
    port: u16,
    socket: Arc<RwLock<Option<UdpSocket>>>,
}

impl MulticastDiscovery {
    pub fn new(group: &str, port: u16) -> Result<Self> {
        let group: Ipv4Addr = group
            .parse()
            .map_err(|_| Error::Discovery("invalid multicast group".into()))?;

        Ok(Self {
            group,
            port,
            socket: Arc::new(RwLock::new(None)),
        })
    }

    async fn ensure_socket(&self) -> Result<()> {
        let mut socket_guard = self.socket.write().await;
        if socket_guard.is_some() {
            return Ok(());
        }

        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| Error::Discovery(format!("socket creation failed: {}", e)))?;

        socket
            .set_reuse_address(true)
            .map_err(|e| Error::Discovery(format!("set_reuse_address failed: {}", e)))?;

        #[cfg(unix)]
        socket
            .set_reuse_port(true)
            .map_err(|e| Error::Discovery(format!("set_reuse_port failed: {}", e)))?;

        socket
            .set_multicast_loop_v4(true)
            .map_err(|e| Error::Discovery(format!("set_multicast_loop failed: {}", e)))?;

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.port);
        socket
            .bind(&addr.into())
            .map_err(|e| Error::Discovery(format!("bind failed: {}", e)))?;

        socket
            .join_multicast_v4(&self.group, &Ipv4Addr::UNSPECIFIED)
            .map_err(|e| Error::Discovery(format!("join_multicast failed: {}", e)))?;

        socket
            .set_nonblocking(true)
            .map_err(|e| Error::Discovery(format!("set_nonblocking failed: {}", e)))?;

        let tokio_socket = UdpSocket::from_std(socket.into())
            .map_err(|e| Error::Discovery(format!("tokio socket conversion failed: {}", e)))?;

        *socket_guard = Some(tokio_socket);
        Ok(())
    }

    async fn send_message(&self, msg_type: u8, data: &[u8]) -> Result<()> {
        self.ensure_socket().await?;

        let socket_guard = self.socket.read().await;
        let socket = socket_guard
            .as_ref()
            .ok_or_else(|| Error::Discovery("socket not initialized".into()))?;

        let mut packet = vec![msg_type];
        packet.extend_from_slice(data);

        let dest = SocketAddr::new(IpAddr::V4(self.group), self.port);
        socket
            .send_to(&packet, dest)
            .await
            .map_err(|e| Error::Discovery(format!("send failed: {}", e)))?;

        debug!("sent multicast message type {}", msg_type);
        Ok(())
    }
}

#[async_trait]
impl ClusterDiscovery for MulticastDiscovery {
    async fn discover(&self) -> Result<Vec<NodeInfo>> {
        self.ensure_socket().await?;

        let socket_guard = self.socket.read().await;
        let socket = socket_guard
            .as_ref()
            .ok_or_else(|| Error::Discovery("socket not initialized".into()))?;

        let mut nodes = Vec::new();
        let mut buf = vec![0u8; 1024];

        loop {
            match tokio::time::timeout(Duration::from_millis(100), socket.recv_from(&mut buf)).await
            {
                Ok(Ok((len, _addr))) => {
                    if len > 1 && buf[0] == DISCOVERY_MSG_ANNOUNCE {
                        if let Ok(node) = serde_json::from_slice::<NodeInfo>(&buf[1..len]) {
                            debug!("discovered node via multicast: {}", node.id);
                            nodes.push(node);
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!("multicast recv error: {}", e);
                    break;
                }
                Err(_) => break,
            }
        }

        Ok(nodes)
    }

    async fn announce(&self, node: &NodeInfo) -> Result<()> {
        let data = serde_json::to_vec(node)
            .map_err(|e| Error::Discovery(format!("serialization failed: {}", e)))?;
        self.send_message(DISCOVERY_MSG_ANNOUNCE, &data).await
    }

    async fn withdraw(&self, node_id: &NodeId) -> Result<()> {
        let data = serde_json::to_vec(node_id)
            .map_err(|e| Error::Discovery(format!("serialization failed: {}", e)))?;
        self.send_message(DISCOVERY_MSG_WITHDRAW, &data).await
    }
}
