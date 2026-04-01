use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::ipp::operations::PrintJobCallback;
use crate::ipp::printer::PrinterState;
use super::http::HttpServer;

pub struct ServerConfig {
    pub port: u16,
    pub storage_dir: PathBuf,
    pub printer_name: String,
    pub callback: Option<Arc<dyn PrintJobCallback>>,
}

pub struct ServerHandle {
    pub shutdown_tx: oneshot::Sender<()>,
    pub local_ip: Ipv4Addr,
    pub port: u16,
}

impl ServerHandle {
    pub fn stop(self) {
        let _ = self.shutdown_tx.send(());
    }

    pub fn printer_uri(&self) -> String {
        format!("ipp://{}:{}/ipp/print", self.local_ip, self.port)
    }
}

/// Get the local LAN IP address (first non-loopback IPv4)
pub fn get_local_ip() -> Ipv4Addr {
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        for addr in addrs {
            if addr.is_loopback() { continue; }
            if let IpAddr::V4(v4) = addr.addr.ip() {
                return v4;
            }
        }
    }
    Ipv4Addr::new(127, 0, 0, 1)
}

pub async fn start(config: ServerConfig) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let local_ip = get_local_ip();
    tracing::info!("Local IP: {}", local_ip);

    std::fs::create_dir_all(&config.storage_dir)?;

    let printer = Arc::new(PrinterState {
        printer_uri: format!("ipp://{}:{}/ipp/print", local_ip, config.port),
        printer_name: config.printer_name.clone(),
        storage_dir: config.storage_dir,
        job_counter: AtomicU32::new(1),
        active_jobs: dashmap::DashMap::new(),
    });

    // Bind the TCP listener HERE so bind errors are caught before returning Ok
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port);
    let listener = TcpListener::bind(addr).await
        .map_err(|e| format!("Failed to bind port {}: {} (try port > 1024)", config.port, e))?;

    tracing::info!("IPP HTTP server bound to {}", addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let http_server = HttpServer::new(printer.clone(), config.callback);

    // mDNS is handled by Android NsdManager on the Kotlin side; not started here.

    // Start HTTP server with the pre-bound listener
    tokio::spawn(async move {
        if let Err(e) = http_server.run_with_listener(listener, shutdown_rx).await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    Ok(ServerHandle {
        shutdown_tx,
        local_ip,
        port: config.port,
    })
}
