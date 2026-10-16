use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use dashmap::DashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Pending = 3,
    Processing = 5,
    Completed = 9,
    Aborted = 7,
    Canceled = 8,
}

/// What the device should turn a received PDF into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Pdf,
    Epub,
}

/// One of the virtual printers served on the shared port. They differ only in
/// resource path, identity and what happens to the document after it lands.
#[derive(Debug, Clone)]
pub struct PrinterProfile {
    pub name: String,
    /// Resource path, without the leading slash (also the mDNS `rp` key).
    pub resource: &'static str,
    /// CUPS/macOS key printers by UUID, so each profile needs its own.
    pub uuid: String,
    pub make_and_model: &'static str,
    pub output: OutputFormat,
}

pub const PDF_PRINTER_UUID: &str = "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e";
pub const EPUB_PRINTER_UUID: &str = "5c1e9f30-8b7d-4a26-b3e4-6d0f2a9c7e15";

/// What this device advertises itself as. Every install picks its own (the
/// app stores it), so several phones on one network don't claim the same
/// mDNS names and printer UUIDs and knock each other out.
#[derive(Debug, Clone)]
pub struct PrinterIdentity {
    pub pdf_name: String,
    pub epub_name: String,
    pub pdf_uuid: String,
    pub epub_uuid: String,
    /// mDNS host name, without ".local."
    pub host_name: String,
}

impl PrinterIdentity {
    /// One fixed identity: "<base>" and "<base> EPUB" with the built-in UUIDs.
    pub fn fixed(base_name: &str) -> Self {
        Self {
            pdf_name: base_name.to_string(),
            epub_name: format!("{} EPUB", base_name),
            pdf_uuid: PDF_PRINTER_UUID.to_string(),
            epub_uuid: EPUB_PRINTER_UUID.to_string(),
            host_name: base_name.to_lowercase().replace(' ', "-"),
        }
    }
}

impl PrinterProfile {
    /// The PDF printer, plus the EPUB printer when enabled. The PDF printer
    /// always comes first: it also answers on `/`.
    pub fn all(identity: &PrinterIdentity, epub_enabled: bool) -> Vec<PrinterProfile> {
        let mut v = vec![PrinterProfile {
            name: identity.pdf_name.clone(),
            resource: "ipp/print",
            uuid: identity.pdf_uuid.clone(),
            make_and_model: "InkPrint Virtual PDF Printer",
            output: OutputFormat::Pdf,
        }];
        if epub_enabled {
            v.push(PrinterProfile {
                name: identity.epub_name.clone(),
                resource: "ipp/epub",
                uuid: identity.epub_uuid.clone(),
                make_and_model: "InkPrint Virtual EPUB Printer",
                output: OutputFormat::Epub,
            });
        }
        v
    }
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: u32,
    pub state: JobState,
    pub name: String,
    pub originating_user: String,
    pub time_created: u64,
    pub file_path: Option<PathBuf>,
    pub size_bytes: u64,
    pub output: OutputFormat,
}

pub struct PrinterState {
    pub profiles: Vec<PrinterProfile>,
    pub storage_dir: PathBuf,
    pub job_counter: AtomicU32,
    pub active_jobs: DashMap<u32, JobInfo>,
}

impl PrinterState {
    pub fn new(identity: &PrinterIdentity, epub_enabled: bool, storage_dir: PathBuf) -> Self {
        Self {
            profiles: PrinterProfile::all(identity, epub_enabled),
            storage_dir,
            job_counter: AtomicU32::new(1),
            active_jobs: DashMap::new(),
        }
    }

    pub fn next_job_id(&self) -> u32 {
        self.job_counter.fetch_add(1, Ordering::SeqCst)
    }

    /// The printer a request path addresses; `/` is the PDF printer, as before
    /// there was more than one.
    pub fn profile_for_path(&self, path: &str) -> Option<&PrinterProfile> {
        let resource = path.trim_start_matches('/');
        if resource.is_empty() {
            return self.profiles.first();
        }
        self.profiles.iter().find(|p| p.resource == resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_map_to_profiles() {
        let s = PrinterState::new(&PrinterIdentity::fixed("InkPrint"), true, PathBuf::new());
        assert_eq!(s.profile_for_path("/").unwrap().output, OutputFormat::Pdf);
        assert_eq!(s.profile_for_path("/ipp/print").unwrap().output, OutputFormat::Pdf);
        let epub = s.profile_for_path("/ipp/epub").unwrap();
        assert_eq!(epub.output, OutputFormat::Epub);
        assert_eq!(epub.name, "InkPrint EPUB");
        assert!(s.profile_for_path("/ipp/other").is_none());
    }

    #[test]
    fn epub_path_is_gone_when_disabled() {
        let s = PrinterState::new(&PrinterIdentity::fixed("InkPrint"), false, PathBuf::new());
        assert!(s.profile_for_path("/ipp/epub").is_none());
        assert_eq!(s.profiles.len(), 1);
    }
}
