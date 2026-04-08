pub mod connection;
pub mod transport;
pub mod discovery;
pub mod identity;
pub mod iroh_transport;

pub use connection::{ConnectionRole, PeerConnection};
pub use transport::{QuicClient, QuicServer};
pub use discovery::LanDiscovery;
pub use identity::DeviceIdentity;
pub use iroh_transport::IrohTransport;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("discovery error: {0}")]
    Discovery(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("timeout")]
    Timeout,
    #[error("peer disconnected")]
    Disconnected,
}
