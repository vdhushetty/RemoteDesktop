use crate::identity::DeviceIdentity;
use crate::NetError;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr};

/// ALPN protocol identifier for remote desktop
pub const RD_ALPN: &[u8] = b"rd/1";

/// Iroh-based transport that handles NAT traversal and relay fallback
pub struct IrohTransport {
    endpoint: Endpoint,
    identity: DeviceIdentity,
}

impl IrohTransport {
    /// Create a new iroh transport with the given device identity.
    /// This connects to the N0 relay network for NAT traversal.
    pub async fn new(identity: DeviceIdentity) -> Result<Self, NetError> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity.secret_key().clone())
            .alpns(vec![RD_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| NetError::Connection(format!("iroh endpoint bind: {e}")))?;

        // Wait for the endpoint to be online (connected to relay, has addresses)
        endpoint.online().await;

        let addr = endpoint.addr();
        tracing::info!(
            device_id = %identity.device_id_short(),
            addr = ?addr,
            "iroh transport online"
        );

        Ok(Self { endpoint, identity })
    }

    /// Get the iroh endpoint for direct use
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get the device identity
    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Get the full endpoint address (for sharing with peers)
    pub fn endpoint_addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Get the device ID (public key) as a string
    pub fn device_id(&self) -> String {
        self.identity.device_id()
    }

    /// Get a short device ID for display
    pub fn device_id_short(&self) -> String {
        self.identity.device_id_short()
    }

    /// Connect to a remote peer by their EndpointAddr
    pub async fn connect(
        &self,
        addr: impl Into<EndpointAddr>,
    ) -> Result<iroh::endpoint::Connection, NetError> {
        let addr = addr.into();
        tracing::info!(
            remote_id = %addr.id.fmt_short(),
            "connecting to peer via iroh"
        );

        let conn = self
            .endpoint
            .connect(addr, RD_ALPN)
            .await
            .map_err(|e| NetError::Connection(format!("iroh connect: {e}")))?;

        tracing::info!(
            remote = %conn.remote_id().fmt_short(),
            "iroh connection established"
        );
        Ok(conn)
    }

    /// Connect to a remote peer by just their public key (relies on discovery/relay)
    pub async fn connect_by_id(
        &self,
        node_id: iroh::PublicKey,
    ) -> Result<iroh::endpoint::Connection, NetError> {
        let addr = EndpointAddr::from(node_id);
        self.connect(addr).await
    }

    /// Accept the next incoming connection
    pub async fn accept(&self) -> Result<iroh::endpoint::Connection, NetError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(NetError::Disconnected)?;

        let conn = incoming
            .await
            .map_err(|e| NetError::Connection(format!("iroh accept: {e}")))?;

        tracing::info!(
            remote = %conn.remote_id().fmt_short(),
            "accepted iroh connection"
        );

        Ok(conn)
    }

    /// Shut down the iroh transport
    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }
}

/// Parse a device ID string back to a PublicKey
pub fn parse_device_id(id_str: &str) -> Result<iroh::PublicKey, NetError> {
    id_str
        .parse()
        .map_err(|e| NetError::Connection(format!("invalid device ID: {e}")))
}
