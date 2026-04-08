use crate::NetError;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

const SERVICE_TYPE: &str = "_rd._udp.local.";
const SERVICE_NAME: &str = "remote-desktop";

/// Discovered device on the local network
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub name: String,
    pub addr: SocketAddr,
    pub os: String,
}

/// Handles mDNS service registration and discovery on the local network
pub struct LanDiscovery {
    daemon: ServiceDaemon,
    devices: Arc<Mutex<HashMap<String, DiscoveredDevice>>>,
}

impl LanDiscovery {
    pub fn new() -> Result<Self, NetError> {
        let daemon =
            ServiceDaemon::new().map_err(|e| NetError::Discovery(format!("mdns init: {e}")))?;

        Ok(Self {
            daemon,
            devices: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Register this device as available for remote connections (used by agent)
    pub fn register(&self, device_name: &str, port: u16, os: &str) -> Result<(), NetError> {
        let host_name = format!("{}.local.", device_name.replace(' ', "-").to_lowercase());

        let properties = [("os", os), ("version", "0.1.0")];

        let service = ServiceInfo::new(
            SERVICE_TYPE,
            device_name,
            &host_name,
            "",
            port,
            &properties[..],
        )
        .map_err(|e| NetError::Discovery(format!("service info: {e}")))?;

        self.daemon
            .register(service)
            .map_err(|e| NetError::Discovery(format!("register: {e}")))?;

        tracing::info!(device_name, port, "registered mDNS service");

        Ok(())
    }

    /// Start browsing for remote desktop agents on the network (used by viewer)
    pub fn start_browsing(&self) -> Result<(), NetError> {
        let receiver = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| NetError::Discovery(format!("browse: {e}")))?;

        let devices = self.devices.clone();

        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let name = info.get_fullname().to_string();

                        if let Some(addr) = info.get_addresses().iter().next() {
                            let socket_addr = SocketAddr::new(*addr, info.get_port());
                            let os = info
                                .get_properties()
                                .get("os")
                                .map(|v| v.val_str().to_string())
                                .unwrap_or_default();

                            let device = DiscoveredDevice {
                                name: info.get_hostname().to_string(),
                                addr: socket_addr,
                                os,
                            };

                            tracing::info!(
                                name = %device.name,
                                addr = %device.addr,
                                os = %device.os,
                                "discovered device"
                            );

                            devices.lock().unwrap().insert(name, device);
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, name) => {
                        tracing::info!(%name, "device removed");
                        devices.lock().unwrap().remove(&name);
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Get the current list of discovered devices
    pub fn devices(&self) -> Vec<DiscoveredDevice> {
        self.devices.lock().unwrap().values().cloned().collect()
    }
}
