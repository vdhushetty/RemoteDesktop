use crate::NetError;
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Device identity based on Ed25519 keypair
#[derive(Clone)]
pub struct DeviceIdentity {
    secret_key: SecretKey,
    device_name: String,
    config_path: PathBuf,
}

/// Persistent identity + trusted peers config
#[derive(Serialize, Deserialize, Default)]
struct IdentityConfig {
    /// Hex-encoded secret key
    secret_key: Option<String>,
    /// Human-readable device name
    device_name: Option<String>,
    /// Trusted peers: device_id -> display name
    trusted_peers: HashMap<String, TrustedPeer>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TrustedPeer {
    pub name: String,
    pub added_at: String,
}

impl DeviceIdentity {
    /// Load or create a device identity from the config directory
    pub fn load_or_create(device_name: Option<String>) -> Result<Self, NetError> {
        let config_dir = config_dir()?;
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| NetError::Connection(format!("create config dir: {e}")))?;

        let config_path = config_dir.join("identity.json");
        let mut config = load_config(&config_path);

        // Load or generate secret key
        let secret_key = if let Some(ref hex) = config.secret_key {
            let bytes = hex::decode(hex)
                .map_err(|e| NetError::Connection(format!("decode secret key: {e}")))?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| NetError::Connection("invalid secret key length".into()))?;
            SecretKey::from_bytes(&bytes)
        } else {
            let key = SecretKey::generate(&mut rand::rng());
            config.secret_key = Some(hex::encode(key.to_bytes()));
            save_config(&config_path, &config)?;
            key
        };

        // Set device name
        let device_name = device_name
            .or(config.device_name.clone())
            .unwrap_or_else(|| {
                hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "unknown".into())
            });

        if config.device_name.as_deref() != Some(&device_name) {
            config.device_name = Some(device_name.clone());
            save_config(&config_path, &config)?;
        }

        let public_key = secret_key.public();
        tracing::info!(
            device_id = %device_id_string(&public_key),
            device_name = %device_name,
            "device identity loaded"
        );

        Ok(Self {
            secret_key,
            device_name,
            config_path,
        })
    }

    pub fn secret_key(&self) -> &SecretKey {
        &self.secret_key
    }

    pub fn public_key(&self) -> iroh::PublicKey {
        self.secret_key.public()
    }

    /// Get a short device ID string for display (first 10 chars of base32)
    pub fn device_id_short(&self) -> String {
        let full = self.public_key().to_string();
        full[..10.min(full.len())].to_string()
    }

    /// Get the full device ID string
    pub fn device_id(&self) -> String {
        device_id_string(&self.public_key())
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Check if a peer is trusted
    pub fn is_trusted(&self, peer_id: &str) -> bool {
        let config = load_config(&self.config_path);
        config.trusted_peers.contains_key(peer_id)
    }

    /// Add a peer to the trusted list
    pub fn trust_peer(&self, peer_id: &str, name: &str) -> Result<(), NetError> {
        let mut config = load_config(&self.config_path);
        config.trusted_peers.insert(
            peer_id.to_string(),
            TrustedPeer {
                name: name.to_string(),
                added_at: chrono_now(),
            },
        );
        save_config(&self.config_path, &config)
    }

    /// Remove a peer from the trusted list
    pub fn untrust_peer(&self, peer_id: &str) -> Result<(), NetError> {
        let mut config = load_config(&self.config_path);
        config.trusted_peers.remove(peer_id);
        save_config(&self.config_path, &config)
    }

    /// Get all trusted peers
    pub fn trusted_peers(&self) -> HashMap<String, TrustedPeer> {
        let config = load_config(&self.config_path);
        config.trusted_peers
    }
}

/// Generate a one-time 6-digit pairing code
pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let code: u32 = rand::rng().random_range(100000..999999);
    format!("{code}")
}

fn device_id_string(key: &iroh::PublicKey) -> String {
    format!("{key}")
}

fn config_dir() -> Result<PathBuf, NetError> {
    dirs::config_dir()
        .map(|p| p.join("remote-desktop"))
        .ok_or_else(|| NetError::Connection("no config directory found".into()))
}

fn load_config(path: &PathBuf) -> IdentityConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(path: &PathBuf, config: &IdentityConfig) -> Result<(), NetError> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| NetError::Connection(format!("serialize config: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| NetError::Connection(format!("write config: {e}")))?;
    Ok(())
}

fn chrono_now() -> String {
    // Simple ISO-ish timestamp without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}
