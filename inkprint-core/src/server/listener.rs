use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::ipp::operations::PrintJobCallback;
use crate::ipp::printer::{PrinterIdentity, PrinterState, PrintDefaults};
use crate::mdns::advertiser::MdnsAdvertiser;
use super::http::HttpServer;

pub struct ServerConfig {
    pub port: u16,
    pub storage_dir: PathBuf,
    pub identity: PrinterIdentity,
    /// Also serve (and advertise) the EPUB printer on /ipp/epub.
    pub epub_enabled: bool,
    /// Colour mode and paper size the printers offer by default.
    pub defaults: PrintDefaults,
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
        &config.identity,
        config.epub_enabled,
        &config.defaults,
        config.storage_dir,
    ));

    // Bind the TCP listener HERE so bind errors are caught before returning Ok.
    // Dual-stack [::] takes IPv6 clients as well (some Wi-Fi networks only pass
    // our mDNS over IPv6, so clients reach us by a link-local address); plain
    // IPv4 where the device has no IPv6.
    let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), config.port);
    let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port);
    let (listener, addr) = match TcpListener::bind(v6).await {
        Ok(l) => (l, v6),
        Err(_) => (TcpListener::bind(v4).await
            .map_err(|e| format!("Failed to bind port {}: {} (try port > 1024)", config.port, e))?, v4),
    };

    tracing::info!("IPP HTTP server bound to {}", addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let (mdns_tx, mdns_rx) = oneshot::channel::<()>();

    let http_server = HttpServer::new(printer.clone(), config.callback);

    // Start mDNS advertiser: broadcasts _ipp._tcp + _universal._sub._ipp._tcp (AirPrint).
    // This is handled in Rust (not Android NsdManager) so that subtype PTR records are
    // correct on all Android versions — NsdManager on API < 33 does not create proper
    // subtype PTR structure needed for macOS AirPrint auto-discovery.
    let mut mdns_addrs = vec![IpAddr::V4(local_ip)];
    mdns_addrs.extend(ipv6_beside(local_ip).into_iter().map(IpAddr::V6));
    tracing::info!("mDNS addresses: {:?}", mdns_addrs);
    let mdns = MdnsAdvertiser::new(printer.profiles.clone(), &config.identity.host_name, mdns_addrs, config.port);
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

/// The IPv6 addresses on the interface that carries `v4`. mdns-sd answers on
/// each interface only with that interface's addresses, so without these the
/// printer is never announced over IPv6 — and some Wi-Fi networks drop IPv4
/// multicast between clients while passing IPv6.
fn ipv6_beside(v4: Ipv4Addr) -> Vec<Ipv6Addr> {
    let ifs = if_addrs::get_if_addrs().unwrap_or_default();
    let Some(name) = ifs.iter().find(|a| a.ip() == IpAddr::V4(v4)).map(|a| a.name.clone()) else {
        return Vec::new();
    };
    ifs.into_iter()
        .filter(|a| a.name == name)
        .filter_map(|a| match a.ip() {
            IpAddr::V6(v6) if !v6.is_loopback() => Some(v6),
            _ => None,
        })
        .collect()
}
