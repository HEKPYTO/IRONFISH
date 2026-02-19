pub mod graphql;
pub mod grpc;
pub mod rest;
mod router;
pub mod ws;
pub use router::{ApiRouter, ApiState, WebSocketConfig};
pub mod proto {
    tonic::include_proto!("chess");
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("chess_descriptor");
}
