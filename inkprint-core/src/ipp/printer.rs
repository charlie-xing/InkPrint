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

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: u32,
    pub state: JobState,
    pub name: String,
    pub originating_user: String,
    pub time_created: u64,
    pub file_path: Option<PathBuf>,
    pub size_bytes: u64,
}

pub struct PrinterState {
    pub printer_name: String,
    pub printer_uri: String,
    pub storage_dir: PathBuf,
    pub job_counter: AtomicU32,
    pub active_jobs: DashMap<u32, JobInfo>,
}

impl PrinterState {
    pub fn new(printer_name: String, ip: &str, port: u16, storage_dir: PathBuf) -> Self {
        Self {
            printer_uri: format!("ipp://{}:{}/ipp/print", ip, port),
            printer_name,
            storage_dir,
            job_counter: AtomicU32::new(1),
            active_jobs: DashMap::new(),
        }
    }

    pub fn next_job_id(&self) -> u32 {
        self.job_counter.fetch_add(1, Ordering::SeqCst)
    }
}
