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
    pub uuid: &'static str,
    pub make_and_model: &'static str,
    pub output: OutputFormat,
}

pub const PDF_PRINTER_UUID: &str = "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e";
pub const EPUB_PRINTER_UUID: &str = "5c1e9f30-8b7d-4a26-b3e4-6d0f2a9c7e15";

impl PrinterProfile {
    /// The PDF printer, plus the EPUB printer when enabled. The PDF printer
    /// always comes first: it also answers on `/`.
    pub fn all(base_name: &str, epub_enabled: bool) -> Vec<PrinterProfile> {
        let mut v = vec![PrinterProfile {
            name: base_name.to_string(),
            resource: "ipp/print",
            uuid: PDF_PRINTER_UUID,
            make_and_model: "InkPrint Virtual PDF Printer",
            output: OutputFormat::Pdf,
        }];
        if epub_enabled {
            v.push(PrinterProfile {
                name: format!("{} EPUB", base_name),
                resource: "ipp/epub",
                uuid: EPUB_PRINTER_UUID,
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
    pub fn new(base_name: &str, epub_enabled: bool, storage_dir: PathBuf) -> Self {
        Self {
            profiles: PrinterProfile::all(base_name, epub_enabled),
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
        let s = PrinterState::new("InkPrint", true, PathBuf::new());
        assert_eq!(s.profile_for_path("/").unwrap().output, OutputFormat::Pdf);
        assert_eq!(s.profile_for_path("/ipp/print").unwrap().output, OutputFormat::Pdf);
        let epub = s.profile_for_path("/ipp/epub").unwrap();
        assert_eq!(epub.output, OutputFormat::Epub);
        assert_eq!(epub.name, "InkPrint EPUB");
        assert!(s.profile_for_path("/ipp/other").is_none());
    }

    #[test]
    fn epub_path_is_gone_when_disabled() {
        let s = PrinterState::new("InkPrint", false, PathBuf::new());
        assert!(s.profile_for_path("/ipp/epub").is_none());
        assert_eq!(s.profiles.len(), 1);
    }
}
