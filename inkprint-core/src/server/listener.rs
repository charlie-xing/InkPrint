use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::ipp::operations::PrintJobCallback;
use crate::ipp::printer::PrinterState;
use crate::mdns::advertiser::MdnsAdvertiser;
use super::http::HttpServer;

pub struct ServerConfig {
    pub port: u16,
    pub storage_dir: PathBuf,
    pub printer_name: String,
    /// Also serve (and advertise) the "<name> EPUB" printer on /ipp/epub.
    pub epub_enabled: bool,
    pub callback: Option<Arc<dyn PrintJobCallback>>,
}

pub struct ServerHandle {
    pub shutdown_tx: oneshot::Sender<()>,
    pub mdns_tx: oneshot::Sender<()>,
    pub local_ip: Ipv4Addr,
    pub port: u16,
}

impl ServerHandle {
    pub fn stop(self) {
        let _ = self.mdns_tx.send(());
        let _ = self.shutdown_tx.send(());
    }

    pub fn printer_uri(&self) -> String {
        format!("ipp://{}:{}/ipp/print", self.local_ip, self.port)
    }
}

/// Get the local LAN IP address (first non-loopback IPv4)
/// Picks the address LAN clients should use. Phones often carry several
/// interfaces at once (Wi-Fi, cellular, VPN, hotspot), so prefer Wi-Fi
/// (`wlan*`), then any private LAN address, then anything non-loopback.
pub fn get_local_ip() -> Ipv4Addr {
    let v4s: Vec<(String, Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.is_loopback())
        .filter_map(|a| match a.addr.ip() {
            IpAddr::V4(v4) => Some((a.name, v4)),
            _ => None,
        })
        .collect();
    v4s.iter()
        .find(|(name, v4)| name.starts_with("wlan") && v4.is_private())
        .or_else(|| v4s.iter().find(|(_, v4)| v4.is_private()))
        .or_else(|| v4s.first())
        .map(|(_, v4)| *v4)
        .unwrap_or(Ipv4Addr::new(127, 0, 0, 1))
}

pub async fn start(config: ServerConfig) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let local_ip = get_local_ip();
    tracing::info!("Local IP: {}", local_ip);

    std::fs::create_dir_all(&config.storage_dir)?;

    let printer = Arc::new(PrinterState::new(
        &config.printer_name,
        config.epub_enabled,
        config.storage_dir,
    ));

    // Bind the TCP listener HERE so bind errors are caught before returning Ok
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port);
    let listener = TcpListener::bind(addr).await
        .map_err(|e| format!("Failed to bind port {}: {} (try port > 1024)", config.port, e))?;

    tracing::info!("IPP HTTP server bound to {}", addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let (mdns_tx, mdns_rx) = oneshot::channel::<()>();

    let http_server = HttpServer::new(printer.clone(), config.callback);

    // Start mDNS advertiser: broadcasts _ipp._tcp + _universal._sub._ipp._tcp (AirPrint).
    // This is handled in Rust (not Android NsdManager) so that subtype PTR records are
    // correct on all Android versions — NsdManager on API < 33 does not create proper
    // subtype PTR structure needed for macOS AirPrint auto-discovery.
    let mdns = MdnsAdvertiser::new(printer.profiles.clone(), &config.printer_name, local_ip, config.port);
    tokio::spawn(async move {
        if let Err(e) = mdns.start(mdns_rx).await {
            tracing::error!("mDNS error: {}", e);
        }
    });

    // Start HTTP server with the pre-bound listener
    tokio::spawn(async move {
        if let Err(e) = http_server.run_with_listener(listener, shutdown_rx).await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    Ok(ServerHandle {
        shutdown_tx,
        mdns_tx,
        local_ip,
        port: config.port,
    })
}
