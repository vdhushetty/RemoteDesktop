use anyhow::Result;
use rd_net::identity::DeviceIdentity;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const RELAY_PORT: u16 = 9877;

/// Simple rendezvous server that helps peers find each other.
/// Agents register with their device ID, viewers look up agents by ID.
/// The actual data relay is handled by iroh's DERP infrastructure.
/// This server only stores device registrations for discovery.

#[derive(Clone)]
struct DeviceRegistry {
    /// device_id -> (device_name, last_seen_timestamp)
    devices: Arc<Mutex<HashMap<String, RegisteredDevice>>>,
}

#[derive(Clone, Debug)]
struct RegisteredDevice {
    name: String,
    os: String,
    last_seen: u64,
}

impl DeviceRegistry {
    fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn register(&self, id: &str, name: &str, os: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.devices.lock().unwrap().insert(
            id.to_string(),
            RegisteredDevice {
                name: name.to_string(),
                os: os.to_string(),
                last_seen: now,
            },
        );
    }

    fn lookup(&self, id: &str) -> Option<RegisteredDevice> {
        self.devices.lock().unwrap().get(id).cloned()
    }

    fn list_online(&self) -> Vec<(String, RegisteredDevice)> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.devices
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| now - d.last_seen < 300) // 5 minute timeout
            .map(|(id, d)| (id.clone(), d.clone()))
            .collect()
    }

    fn cleanup_stale(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.devices.lock().unwrap().retain(|_, d| now - d.last_seen < 600);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let registry = DeviceRegistry::new();

    // Periodic cleanup
    let reg_cleanup = registry.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            reg_cleanup.cleanup_stale();
        }
    });

    // Simple TCP server for device registration/lookup
    let addr: SocketAddr = format!("0.0.0.0:{RELAY_PORT}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(%addr, "rd-relay rendezvous server listening");
    println!("\n========================================");
    println!("  Relay Server: {addr}");
    println!("  Protocol: TCP registration/lookup");
    println!("  Data relay: via iroh DERP network");
    println!("========================================\n");

    loop {
        let (mut stream, peer) = listener.accept().await?;
        let reg = registry.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            match stream.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let parts: Vec<&str> = request.trim().split('|').collect();

                    let response = match parts.first().map(|s| *s) {
                        Some("REGISTER") if parts.len() >= 4 => {
                            // REGISTER|device_id|device_name|os
                            reg.register(parts[1], parts[2], parts[3]);
                            tracing::info!(
                                device_id = %&parts[1][..10.min(parts[1].len())],
                                name = parts[2],
                                "device registered"
                            );
                            "OK\n".to_string()
                        }
                        Some("LOOKUP") if parts.len() >= 2 => {
                            // LOOKUP|device_id
                            match reg.lookup(parts[1]) {
                                Some(dev) => format!("FOUND|{}|{}\n", dev.name, dev.os),
                                None => "NOT_FOUND\n".to_string(),
                            }
                        }
                        Some("LIST") => {
                            let devices = reg.list_online();
                            let list: Vec<String> = devices
                                .iter()
                                .map(|(id, d)| format!("{}|{}|{}", &id[..10.min(id.len())], d.name, d.os))
                                .collect();
                            if list.is_empty() {
                                "EMPTY\n".to_string()
                            } else {
                                format!("DEVICES|{}\n", list.join(";"))
                            }
                        }
                        _ => "ERROR|unknown command\n".to_string(),
                    };

                    let _ = stream.write_all(response.as_bytes()).await;
                }
                _ => {}
            }
        });
    }
}
