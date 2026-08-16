use crate::identity::DeviceIdentity;
use crate::NetError;
use base64::Engine;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr};
use std::time::Duration;

/// ALPN protocol identifier for remote desktop
pub const RD_ALPN: &[u8] = b"rd/1";

/// Connection timeout
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Iroh-based transport that handles NAT traversal and relay fallback
pub struct IrohTransport {
    endpoint: Endpoint,
    identity: DeviceIdentity,
}

impl IrohTransport {
    pub async fn new(identity: DeviceIdentity) -> Result<Self, NetError> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity.secret_key().clone())
            .alpns(vec![RD_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| NetError::Connection(format!("iroh endpoint bind: {e}")))?;

        // Wait for relay connectivity with a timeout — don't block forever if
        // relay servers are unreachable (the endpoint is still usable without them)
        match tokio::time::timeout(Duration::from_secs(10), endpoint.online()).await {
            Ok(_) => tracing::info!(device_id = %identity.device_id_short(), "iroh transport online"),
            Err(_) => tracing::warn!(device_id = %identity.device_id_short(), "iroh online timed out after 10s, continuing anyway"),
        }

        Ok(Self { endpoint, identity })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Get the full endpoint address
    pub fn endpoint_addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Get a shareable connection ticket (base64-encoded full address with relay URLs)
    pub fn connection_ticket(&self) -> String {
        let addr = self.endpoint.addr();
        let json = serde_json::to_vec(&addr).unwrap_or_default();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&json)
    }

    /// Get the short device ID for display
    pub fn device_id(&self) -> String {
        self.identity.device_id()
    }

    pub fn device_id_short(&self) -> String {
        self.identity.device_id_short()
    }

    /// Connect to a peer using a connection ticket (full address)
    pub async fn connect_by_ticket(
        &self,
        ticket: &str,
    ) -> Result<iroh::endpoint::Connection, NetError> {
        let addr = parse_connection_ticket(ticket)?;
        self.connect_with_timeout(addr).await
    }

    /// Connect to a peer by just their public key (slower, relies on discovery)
    pub async fn connect_by_id(
        &self,
        node_id: iroh::PublicKey,
    ) -> Result<iroh::endpoint::Connection, NetError> {
        let addr = EndpointAddr::from(node_id);
        self.connect_with_timeout(addr).await
    }

    /// Connect with a timeout
    async fn connect_with_timeout(
        &self,
        addr: EndpointAddr,
    ) -> Result<iroh::endpoint::Connection, NetError> {
        tracing::info!(
            remote_id = %addr.id.fmt_short(),
            addrs = ?addr.addrs,
            "connecting to peer via iroh"
        );

        let connect_fut = self.endpoint.connect(addr, RD_ALPN);

        match tokio::time::timeout(CONNECT_TIMEOUT, connect_fut).await {
            Ok(Ok(conn)) => {
                tracing::info!(remote = %conn.remote_id().fmt_short(), "connected");
                Ok(conn)
            }
            Ok(Err(e)) => Err(NetError::Connection(format!("iroh connect: {e}"))),
            Err(_) => Err(NetError::Connection(
                "Connection timed out after 30s. Check that:\n\
                 - The remote machine is running and has this app open\n\
                 - Both machines have internet access\n\
                 - Firewalls allow UDP traffic (Windows: check Windows Defender Firewall)\n\
                 - The connection code was copied completely\n\
                 - If on the same network, try connecting by IP address instead".to_string()
            )),
        }
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

        tracing::info!(remote = %conn.remote_id().fmt_short(), "accepted connection");
        Ok(conn)
    }

    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }
}

/// Parse a connection ticket (base64-encoded EndpointAddr) back to an EndpointAddr.
/// Strips ALL whitespace (not just trim) to handle newlines/spaces from copy-paste
/// through messaging apps.
pub fn parse_connection_ticket(ticket: &str) -> Result<EndpointAddr, NetError> {
    let cleaned: String = ticket.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&cleaned)
        .map_err(|e| NetError::Connection(format!("invalid ticket encoding: {e}")))?;
    let addr: EndpointAddr = serde_json::from_slice(&bytes)
        .map_err(|e| NetError::Connection(format!("invalid ticket data: {e}")))?;
    Ok(addr)
}

/// Parse a device ID string (hex public key) back to a PublicKey
pub fn parse_device_id(id_str: &str) -> Result<iroh::PublicKey, NetError> {
    id_str
        .parse()
        .map_err(|e| NetError::Connection(format!("invalid device ID: {e}")))
}

/// Check if a string looks like a connection ticket (base64) vs a device ID (hex).
/// Tries actual base64 decode rather than heuristics to avoid false positives.
pub fn is_connection_ticket(s: &str) -> bool {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    // Tickets (base64 JSON) are much longer than device IDs (~64 hex chars)
    cleaned.len() > 80
        && base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&cleaned)
            .is_ok()
}
