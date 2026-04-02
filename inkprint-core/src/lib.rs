uniffi::include_scaffolding!("inkprint");

pub mod ipp;
pub mod server;
pub mod mdns;

use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use server::listener::{ServerConfig, ServerHandle, start, get_local_ip as inner_get_local_ip};
use ipp::operations::PrintJobCallback;

/// UniFFI callback interface — implemented by Kotlin
pub trait PrintJobListener: Send + Sync {
    fn on_job_received(&self, job_id: u32, file_path: String, file_name: String, size_bytes: u64);
}

/// Adapter: wrap PrintJobListener as a PrintJobCallback for the core
struct ListenerCallback(Arc<dyn PrintJobListener>);

impl PrintJobCallback for ListenerCallback {
    fn on_job_received(&self, job_id: u32, file_path: String, file_name: String, size_bytes: u64) {
        self.0.on_job_received(job_id, file_path, file_name, size_bytes);
    }
}

static SERVER_HANDLE: Lazy<Mutex<Option<ServerHandle>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "android")]
fn init_android_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("inkprint-rs"),
    );
    // Bridge tracing:: → log:: → android_logger so tracing::error!/info! appear in logcat
    tracing_log::LogTracer::init().ok();
}

pub fn start_server(
    port: u16,
    storage_path: String,
    printer_name: String,
    listener: Option<Arc<dyn PrintJobListener>>,
) -> bool {
    #[cfg(target_os = "android")]
    init_android_logging();

    let mut handle = SERVER_HANDLE.lock().unwrap();
    if handle.is_some() {
        tracing::warn!("Server already running");
        return false;
    }

    let callback: Option<Arc<dyn PrintJobCallback>> = listener
        .map(|l| Arc::new(ListenerCallback(l)) as Arc<dyn PrintJobCallback>);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("Failed to create runtime: {}", e);
            return false;
        }
    };

    let config = ServerConfig {
        port,
        storage_dir: std::path::PathBuf::from(&storage_path),
        printer_name,
        callback,
    };

    match rt.block_on(start(config)) {
        Ok(h) => {
            *handle = Some(h);
            std::mem::forget(rt);
            true
        }
        Err(e) => {
            tracing::error!("Failed to start server: {}", e);
            false
        }
    }
}

pub fn stop_server() -> bool {
    let mut handle = SERVER_HANDLE.lock().unwrap();
    if let Some(h) = handle.take() {
        h.stop();
        true
    } else {
        false
    }
}

pub fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub fn get_local_ip() -> String {
    inner_get_local_ip().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_version() {
        assert_eq!(get_version(), "0.1.0");
    }

    #[test]
    fn test_get_local_ip() {
        let ip = get_local_ip();
        assert!(!ip.is_empty());
    }
}
