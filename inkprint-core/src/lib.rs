uniffi::include_scaffolding!("inkprint");

pub mod ipp;
pub mod server;
pub mod mdns;

use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use server::listener::{ServerConfig, ServerHandle, start, get_local_ip as inner_get_local_ip};
use ipp::operations::PrintJobCallback;
pub use ipp::printer::OutputFormat;

/// UniFFI callback interface — implemented by Kotlin
pub trait PrintJobListener: Send + Sync {
    fn on_job_received(
        &self,
        job_id: u32,
        file_path: String,
        file_name: String,
        size_bytes: u64,
        output: OutputFormat,
    );
}

/// Adapter: wrap PrintJobListener as a PrintJobCallback for the core
struct ListenerCallback(Arc<dyn PrintJobListener>);

impl PrintJobCallback for ListenerCallback {
    fn on_job_received(
        &self,
        job_id: u32,
        file_path: String,
        file_name: String,
        size_bytes: u64,
        output: OutputFormat,
    ) {
        self.0.on_job_received(job_id, file_path, file_name, size_bytes, output);
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
    epub_enabled: bool,
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
        epub_enabled,
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

pub struct ConversionOptions {
    pub title: String,
    pub pdfium_lib: String,
    pub ort_lib: String,
    pub models_dir: Option<String>,
    pub ocr: bool,
    pub threads: u32,
}

pub struct ConversionStats {
    pub pages: u32,
    pub ocr_pages: u32,
    pub image_pages: u32,
    pub layout_model: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// UniFFI callback interface — implemented by Kotlin
pub trait ConversionProgress: Send + Sync {
    fn on_page(&self, done: u32, total: u32);
}

pub fn epub_supported() -> bool {
    cfg!(feature = "epub")
}

#[cfg(feature = "epub")]
pub fn convert_pdf_to_epub(
    pdf_path: String,
    epub_path: String,
    options: ConversionOptions,
    progress: Option<Arc<dyn ConversionProgress>>,
) -> Result<ConversionStats, ConvertError> {
    #[cfg(target_os = "android")]
    init_android_logging();

    struct Forward(Arc<dyn ConversionProgress>);
    impl inkprint_epub::Progress for Forward {
        fn on_page(&self, done: u32, total: u32) {
            self.0.on_page(done, total);
        }
    }

    let opts = inkprint_epub::Options {
        title: options.title,
        pdfium_lib: options.pdfium_lib,
        ort_lib: options.ort_lib,
        models_dir: options.models_dir.map(std::path::PathBuf::from),
        ocr: options.ocr,
        threads: options.threads.max(1) as usize,
    };
    let forward = progress.map(Forward);
    let started = std::time::Instant::now();
    match inkprint_epub::convert(
        pdf_path.as_ref(),
        epub_path.as_ref(),
        &opts,
        forward.as_ref().map(|f| f as &dyn inkprint_epub::Progress),
    ) {
        Ok(s) => {
            tracing::info!("EPUB: {} -> {} in {:.1}s, {:?}", pdf_path, epub_path, started.elapsed().as_secs_f64(), s);
            Ok(ConversionStats {
                pages: s.pages,
                ocr_pages: s.ocr_pages,
                image_pages: s.image_pages,
                layout_model: s.layout_model,
            })
        }
        Err(inkprint_epub::Error::Cancelled) => Err(ConvertError::Cancelled),
        Err(e) => {
            tracing::error!("EPUB conversion of {} failed: {}", pdf_path, e);
            Err(ConvertError::Failed(e.to_string()))
        }
    }
}

#[cfg(not(feature = "epub"))]
pub fn convert_pdf_to_epub(
    _pdf_path: String,
    _epub_path: String,
    _options: ConversionOptions,
    _progress: Option<Arc<dyn ConversionProgress>>,
) -> Result<ConversionStats, ConvertError> {
    Err(ConvertError::Failed("this build has no EPUB support".into()))
}

pub fn cancel_conversion() {
    #[cfg(feature = "epub")]
    inkprint_epub::cancel();
}

pub fn release_models() {
    #[cfg(feature = "epub")]
    inkprint_epub::release_models();
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
