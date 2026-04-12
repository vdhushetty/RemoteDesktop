use crate::NetError;
use quinn::{ClientConfig, Endpoint, ServerConfig};
use std::net::SocketAddr;
use std::sync::Arc;

const ALPN_PROTOCOL: &[u8] = b"rd/1";

/// QUIC server that listens for incoming connections (used by rd-agent)
pub struct QuicServer {
    endpoint: Endpoint,
    local_addr: SocketAddr,
}

impl QuicServer {
    /// Create a new QUIC server bound to the given address
    pub async fn bind(addr: SocketAddr) -> Result<Self, NetError> {
        let (server_config, _cert_der) = generate_self_signed_config()
            .map_err(|e| NetError::Tls(format!("generate config: {e}")))?;

        let endpoint = Endpoint::server(server_config, addr)
            .map_err(|e| NetError::Transport(format!("bind: {e}")))?;

        let local_addr = endpoint
            .local_addr()
            .map_err(|e| NetError::Transport(format!("local_addr: {e}")))?;

        tracing::info!(%local_addr, "QUIC server listening");

        Ok(Self {
            endpoint,
            local_addr,
        })
    }

    /// Accept the next incoming connection
    pub async fn accept(&self) -> Result<quinn::Connection, NetError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(NetError::Disconnected)?;

        let conn = incoming
            .await
            .map_err(|e| NetError::Connection(format!("accept: {e}")))?;

        tracing::info!(
            remote = %conn.remote_address(),
            "accepted connection"
        );

        Ok(conn)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

/// QUIC client that connects to a server (used by rd-viewer)
pub struct QuicClient {
    endpoint: Endpoint,
}

impl QuicClient {
    pub fn new() -> Result<Self, NetError> {
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())
            .map_err(|e| NetError::Transport(format!("client bind: {e}")))?;

        // Configure client to accept self-signed certificates (for development)
        let rustls_config = SkipServerVerification::new();
        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(rustls_config)
                .map_err(|e| NetError::Tls(format!("quic client config: {e}")))?,
        ));
        endpoint.set_default_client_config(client_config);

        Ok(Self { endpoint })
    }

    /// Connect to a remote QUIC server
    pub async fn connect(&self, addr: SocketAddr) -> Result<quinn::Connection, NetError> {
        tracing::info!(%addr, "connecting to server");

        let conn = self
            .endpoint
            .connect(addr, "remote-desktop")
            .map_err(|e| NetError::Connection(format!("connect: {e}")))?
            .await
            .map_err(|e| NetError::Connection(format!("connection: {e}")))?;

        tracing::info!(%addr, "connected");

        Ok(conn)
    }
}

/// Generate self-signed TLS config for QUIC
fn generate_self_signed_config() -> anyhow::Result<(ServerConfig, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["remote-desktop".into()])?;
    let cert_der = cert.cert.der().to_vec();
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

    let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der.clone())];

    // Build rustls server config with ALPN
    let mut rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der.into())?;
    rustls_config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];

    let mut server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(rustls_config)?
    ));

    // Configure transport for low latency
    let transport = Arc::get_mut(&mut server_config.transport).unwrap();
    transport.max_concurrent_bidi_streams(4u8.into());
    transport.max_concurrent_uni_streams(4u8.into());
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(2)));

    Ok((server_config, cert_der))
}

/// Skip server certificate verification (development only)
/// TODO: Replace with proper certificate verification for production
#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> rustls::ClientConfig {
        let mut config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Self))
            .with_no_client_auth();

        config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];
        config
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}
