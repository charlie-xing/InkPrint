use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::*;
use super::printer::{PrinterState, JobInfo, JobState};
use super::response::{IppResponseBuilder, serialize_response, standard_operation_attrs};

/// Callback when a print job completes
pub trait PrintJobCallback: Send + Sync {
    fn on_job_received(&self, job_id: u32, file_path: String, file_name: String, size_bytes: u64);
}

pub fn dispatch(
    request: &IppRequest,
    printer: &Arc<PrinterState>,
    callback: Option<&dyn PrintJobCallback>,
) -> Vec<u8> {
    let response = match request.operation_id {
        IppOperationId::GetPrinterAttributes => {
            handle_get_printer_attributes(request, printer)
        }
        IppOperationId::PrintJob => {
            handle_print_job(request, printer, callback)
        }
        IppOperationId::GetJobAttributes => {
            handle_get_job_attributes(request, printer)
        }
        IppOperationId::ValidateJob => {
            handle_validate_job(request)
        }
        _ => {
            // Operation not supported
            let mut op = standard_operation_attrs(request.request_id);
            op.add(IppAttribute::new(
                "status-message",
                IppValue::TextWithoutLanguage("server-error-operation-not-supported".to_string()),
            ));
            IppResponseBuilder::new(IppStatusCode::ServerErrorOperationNotSupported, request.request_id)
                .add_group(op)
                .build()
        }
    };
    serialize_response(&response)
}

fn handle_get_printer_attributes(
    request: &IppRequest,
    printer: &Arc<PrinterState>,
) -> IppResponse {
    // Determine which attributes were requested
    let requested: Option<Vec<String>> = request.get_operation_attributes()
        .and_then(|g| {
            let attrs: Vec<String> = g.attributes.iter()
                .filter(|a| a.name == "requested-attributes")
                .flat_map(|a| a.values.iter())
                .filter_map(|v| match v {
                    IppValue::Keyword(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();
            if attrs.is_empty() { None } else { Some(attrs) }
        });

    let want = |name: &str| -> bool {
        match &requested {
            None => true, // no filter = return all
            Some(list) => list.iter().any(|r| r == name || r == "all"),
        }
    };

    let op = standard_operation_attrs(request.request_id);

    let mut printer_group = IppAttributeGroup::new(DelimiterTag::PrinterAttributes);

    if want("printer-uri-supported") {
        printer_group.add(IppAttribute::new(
            "printer-uri-supported",
            IppValue::Uri(printer.printer_uri.clone()),
        ));
    }
    if want("uri-security-supported") {
        printer_group.add(IppAttribute::new(
            "uri-security-supported",
            IppValue::Keyword("none".to_string()),
        ));
    }
    if want("uri-authentication-supported") {
        printer_group.add(IppAttribute::new(
            "uri-authentication-supported",
            IppValue::Keyword("none".to_string()),
        ));
    }
    if want("printer-name") {
        printer_group.add(IppAttribute::new(
            "printer-name",
            IppValue::NameWithoutLanguage(printer.printer_name.clone()),
        ));
    }
    if want("printer-make-and-model") {
        printer_group.add(IppAttribute::new(
            "printer-make-and-model",
            IppValue::TextWithoutLanguage("InkPrint Virtual PDF Printer".to_string()),
        ));
    }
    if want("printer-state") {
        printer_group.add(IppAttribute::new(
            "printer-state",
            IppValue::Enum(3), // idle
        ));
    }
    if want("printer-state-reasons") {
        printer_group.add(IppAttribute::new(
            "printer-state-reasons",
            IppValue::Keyword("none".to_string()),
        ));
    }
    if want("ipp-versions-supported") {
        printer_group.add(IppAttribute::new_multi(
            "ipp-versions-supported",
            vec![
                IppValue::Keyword("1.0".to_string()),
                IppValue::Keyword("1.1".to_string()),
                IppValue::Keyword("2.0".to_string()),
            ],
        ));
    }
    if want("ipp-features-supported") {
        printer_group.add(IppAttribute::new_multi(
            "ipp-features-supported",
            vec![
                IppValue::Keyword("ipp-everywhere".to_string()),
                IppValue::Keyword("airprint-2.0".to_string()),
            ],
        ));
    }
    if want("operations-supported") {
        printer_group.add(IppAttribute::new_multi(
            "operations-supported",
            vec![
                IppValue::Enum(0x0002), // Print-Job
                IppValue::Enum(0x0004), // Validate-Job
                IppValue::Enum(0x0009), // Get-Job-Attributes
                IppValue::Enum(0x000B), // Get-Printer-Attributes
            ],
        ));
    }
    if want("charset-configured") {
        printer_group.add(IppAttribute::new(
            "charset-configured",
            IppValue::Charset("utf-8".to_string()),
        ));
    }
    if want("charset-supported") {
        printer_group.add(IppAttribute::new(
            "charset-supported",
            IppValue::Charset("utf-8".to_string()),
        ));
    }
    if want("natural-language-configured") {
        printer_group.add(IppAttribute::new(
            "natural-language-configured",
            IppValue::NaturalLanguage("en".to_string()),
        ));
    }
    if want("generated-natural-language-supported") {
        printer_group.add(IppAttribute::new(
            "generated-natural-language-supported",
            IppValue::NaturalLanguage("en".to_string()),
        ));
    }
    if want("document-format-default") {
        printer_group.add(IppAttribute::new(
            "document-format-default",
            IppValue::MimeMediaType("application/pdf".to_string()),
        ));
    }
    if want("document-format-supported") {
        printer_group.add(IppAttribute::new_multi(
            "document-format-supported",
            vec![
                IppValue::MimeMediaType("application/pdf".to_string()),
                IppValue::MimeMediaType("image/urf".to_string()),
                IppValue::MimeMediaType("image/pwg-raster".to_string()),
                IppValue::MimeMediaType("image/jpeg".to_string()),
            ],
        ));
    }
    if want("pdf-versions-supported") {
        printer_group.add(IppAttribute::new(
            "pdf-versions-supported",
            IppValue::Keyword("adobe-1.4".to_string()),
        ));
    }
    if want("printer-is-accepting-jobs") {
        printer_group.add(IppAttribute::new(
            "printer-is-accepting-jobs",
            IppValue::Boolean(true),
        ));
    }
    if want("queued-job-count") {
        printer_group.add(IppAttribute::new(
            "queued-job-count",
            IppValue::Integer(0),
        ));
    }
    if want("pdl-override-supported") {
        printer_group.add(IppAttribute::new(
            "pdl-override-supported",
            IppValue::Keyword("not-attempted".to_string()),
        ));
    }
    if want("printer-up-time") {
        let uptime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i32;
        printer_group.add(IppAttribute::new(
            "printer-up-time",
            IppValue::Integer(uptime),
        ));
    }
    if want("compression-supported") {
        printer_group.add(IppAttribute::new(
            "compression-supported",
            IppValue::Keyword("none".to_string()),
        ));
    }
    if want("color-supported") {
        printer_group.add(IppAttribute::new(
            "color-supported",
            IppValue::Boolean(false),
        ));
    }
    if want("sides-supported") {
        printer_group.add(IppAttribute::new(
            "sides-supported",
            IppValue::Keyword("one-sided".to_string()),
        ));
    }
    if want("sides-default") {
        printer_group.add(IppAttribute::new(
            "sides-default",
            IppValue::Keyword("one-sided".to_string()),
        ));
    }
    if want("print-color-mode-default") {
        printer_group.add(IppAttribute::new(
            "print-color-mode-default",
            IppValue::Keyword("monochrome".to_string()),
        ));
    }
    if want("print-color-mode-supported") {
        printer_group.add(IppAttribute::new(
            "print-color-mode-supported",
            IppValue::Keyword("monochrome".to_string()),
        ));
    }
    if want("orientation-requested-default") {
        printer_group.add(IppAttribute::new(
            "orientation-requested-default",
            IppValue::Enum(3), // portrait
        ));
    }
    if want("orientation-requested-supported") {
        printer_group.add(IppAttribute::new_multi(
            "orientation-requested-supported",
            vec![
                IppValue::Enum(3), // portrait
                IppValue::Enum(4), // landscape
            ],
        ));
    }
    if want("output-bin-default") {
        printer_group.add(IppAttribute::new(
            "output-bin-default",
            IppValue::Keyword("face-up".to_string()),
        ));
    }
    if want("output-bin-supported") {
        printer_group.add(IppAttribute::new(
            "output-bin-supported",
            IppValue::Keyword("face-up".to_string()),
        ));
    }
    if want("printer-resolution-default") {
        printer_group.add(IppAttribute::new(
            "printer-resolution-default",
            IppValue::Resolution { cross_feed: 300, feed: 300, units: 3 },
        ));
    }
    if want("printer-resolution-supported") {
        printer_group.add(IppAttribute::new(
            "printer-resolution-supported",
            IppValue::Resolution { cross_feed: 300, feed: 300, units: 3 },
        ));
    }
    if want("multiple-document-jobs-supported") {
        printer_group.add(IppAttribute::new(
            "multiple-document-jobs-supported",
            IppValue::Boolean(false),
        ));
    }
    if want("job-creation-attributes-supported") {
        printer_group.add(IppAttribute::new_multi(
            "job-creation-attributes-supported",
            vec![
                IppValue::Keyword("copies".to_string()),
                IppValue::Keyword("document-format".to_string()),
                IppValue::Keyword("job-name".to_string()),
                IppValue::Keyword("media".to_string()),
                IppValue::Keyword("media-col".to_string()),
                IppValue::Keyword("orientation-requested".to_string()),
                IppValue::Keyword("print-color-mode".to_string()),
                IppValue::Keyword("print-quality".to_string()),
                IppValue::Keyword("printer-resolution".to_string()),
                IppValue::Keyword("sides".to_string()),
            ],
        ));
    }
    if want("printer-info") {
        printer_group.add(IppAttribute::new(
            "printer-info",
            IppValue::TextWithoutLanguage("InkPrint Virtual PDF Printer for e-ink reader".to_string()),
        ));
    }
    if want("printer-location") {
        printer_group.add(IppAttribute::new(
            "printer-location",
            IppValue::TextWithoutLanguage("".to_string()),
        ));
    }
    if want("printer-more-info") {
        let host_port = printer.printer_uri
            .trim_start_matches("ipp://")
            .split('/')
            .next()
            .unwrap_or("localhost");
        printer_group.add(IppAttribute::new(
            "printer-more-info",
            IppValue::Uri(format!("http://{}/", host_port)),
        ));
    }
    if want("media-default") {
        printer_group.add(IppAttribute::new(
            "media-default",
            IppValue::Keyword("iso_a4_210x297mm".to_string()),
        ));
    }
    if want("media-supported") {
        printer_group.add(IppAttribute::new_multi(
            "media-supported",
            vec![
                IppValue::Keyword("iso_a4_210x297mm".to_string()),
                IppValue::Keyword("na_letter_8.5x11in".to_string()),
            ],
        ));
    }
    if want("media-col-default") {
        printer_group.add(IppAttribute::new(
            "media-col-default",
            media_col_a4(),
        ));
    }
    if want("media-col-database") {
        printer_group.add(IppAttribute::new_multi(
            "media-col-database",
            vec![media_col_a4(), media_col_letter()],
        ));
    }
    if want("media-col-ready") {
        printer_group.add(IppAttribute::new(
            "media-col-ready",
            media_col_a4(),
        ));
    }
    if want("media-ready") {
        printer_group.add(IppAttribute::new(
            "media-ready",
            IppValue::Keyword("iso_a4_210x297mm".to_string()),
        ));
    }
    if want("copies-default") {
        printer_group.add(IppAttribute::new("copies-default", IppValue::Integer(1)));
    }
    if want("copies-supported") {
        printer_group.add(IppAttribute::new("copies-supported", IppValue::RangeOfInteger { lower: 1, upper: 1 }));
    }
    if want("page-ranges-supported") {
        printer_group.add(IppAttribute::new("page-ranges-supported", IppValue::Boolean(false)));
    }
    if want("print-quality-default") {
        printer_group.add(IppAttribute::new("print-quality-default", IppValue::Enum(4))); // normal
    }
    if want("print-quality-supported") {
        printer_group.add(IppAttribute::new("print-quality-supported", IppValue::Enum(4)));
    }
    // Required by CUPS IPP Everywhere / lpadmin -m everywhere
    if want("printer-uuid") {
        printer_group.add(IppAttribute::new(
            "printer-uuid",
            IppValue::Uri("urn:uuid:a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e".to_string()),
        ));
    }
    if want("urf-supported") {
        printer_group.add(IppAttribute::new_multi(
            "urf-supported",
            vec![
                IppValue::Keyword("CP1".to_string()),
                IppValue::Keyword("W8".to_string()),
                IppValue::Keyword("RS300".to_string()),
            ],
        ));
    }
    if want("pwg-raster-document-resolution-supported") {
        printer_group.add(IppAttribute::new(
            "pwg-raster-document-resolution-supported",
            IppValue::Resolution { cross_feed: 300, feed: 300, units: 3 },
        ));
    }
    if want("pwg-raster-document-sheet-back") {
        printer_group.add(IppAttribute::new(
            "pwg-raster-document-sheet-back",
            IppValue::Keyword("normal".to_string()),
        ));
    }
    if want("pwg-raster-document-type-supported") {
        printer_group.add(IppAttribute::new(
            "pwg-raster-document-type-supported",
            IppValue::Keyword("sgray-8".to_string()),
        ));
    }

    IppResponseBuilder::new(IppStatusCode::SuccessfulOk, request.request_id)
        .add_group(op)
        .add_group(printer_group)
        .build()
}

fn handle_print_job(
    request: &IppRequest,
    printer: &Arc<PrinterState>,
    callback: Option<&dyn PrintJobCallback>,
) -> IppResponse {
    let op_attrs = match request.get_operation_attributes() {
        Some(g) => g,
        None => {
            return error_response(request.request_id, IppStatusCode::ClientErrorBadRequest, "Missing operation-attributes");
        }
    };

    let job_name = match op_attrs.get("job-name") {
        Some(IppValue::NameWithoutLanguage(s)) | Some(IppValue::TextWithoutLanguage(s)) => s.clone(),
        _ => "Untitled".to_string(),
    };

    let requesting_user = match op_attrs.get("requesting-user-name") {
        Some(IppValue::NameWithoutLanguage(s)) | Some(IppValue::TextWithoutLanguage(s)) => s.clone(),
        _ => "unknown".to_string(),
    };

    let doc_format = match op_attrs.get("document-format") {
        Some(IppValue::MimeMediaType(s)) => s.clone(),
        _ => "application/octet-stream".to_string(),
    };

    // Only accept PDF (and octet-stream as a generic fallback).
    // Raster formats (image/urf, image/pwg-raster, image/jpeg) are declared in
    // document-format-supported for IPP Everywhere compliance, but we don't actually
    // store them. Returning ClientErrorDocumentFormatNotSupported causes well-behaved
    // clients (macOS, iOS) to retry with document-format-default (application/pdf).
    match doc_format.as_str() {
        "application/pdf" | "application/octet-stream" => {}
        _ => {
            return error_response(
                request.request_id,
                IppStatusCode::ClientErrorDocumentFormatNotSupported,
                "Only application/pdf is accepted; retry with document-format-default",
            );
        }
    }

    if request.document_data.is_empty() {
        return error_response(request.request_id, IppStatusCode::ClientErrorBadRequest, "No document data");
    }

    let job_id = printer.next_job_id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Sanitize job name for filename
    let safe_name: String = job_name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .take(64)
        .collect();

    let filename = format!("{}_{}_{}.pdf", now, job_id, safe_name);
    let file_path = printer.storage_dir.join(&filename);

    let size_bytes = request.document_data.len() as u64;

    // Check available disk space (require at least 2x document size + 10MB headroom)
    let required_space = size_bytes * 2 + 10 * 1024 * 1024;
    if let Some(avail) = get_available_space(&printer.storage_dir) {
        if avail < required_space {
            tracing::error!("Insufficient disk space: {} available, {} required", avail, required_space);
            return error_response(
                request.request_id,
                IppStatusCode::ServerErrorInternalError,
                "Insufficient storage space",
            );
        }
    }

    // Write document to disk
    if let Err(e) = std::fs::write(&file_path, &request.document_data) {
        tracing::error!("Failed to write document: {}", e);
        return error_response(request.request_id, IppStatusCode::ServerErrorInternalError, "Failed to save document");
    }

    tracing::info!("Saved print job {} -> {:?} ({} bytes)", job_id, file_path, size_bytes);

    // Store job info
    printer.active_jobs.insert(job_id, JobInfo {
        id: job_id,
        state: JobState::Completed,
        name: job_name.clone(),
        originating_user: requesting_user,
        time_created: now,
        file_path: Some(file_path.clone()),
        size_bytes,
    });

    // Notify callback
    if let Some(cb) = callback {
        cb.on_job_received(
            job_id,
            file_path.to_string_lossy().to_string(),
            filename.clone(),
            size_bytes,
        );
    }

    let mut op = standard_operation_attrs(request.request_id);
    op.add(IppAttribute::new(
        "status-message",
        IppValue::TextWithoutLanguage("successful-ok".to_string()),
    ));

    let mut job_group = IppAttributeGroup::new(DelimiterTag::JobAttributes);
    job_group.add(IppAttribute::new("job-id", IppValue::Integer(job_id as i32)));
    job_group.add(IppAttribute::new(
        "job-uri",
        IppValue::Uri(format!("{}/jobs/{}", printer.printer_uri, job_id)),
    ));
    job_group.add(IppAttribute::new("job-state", IppValue::Enum(JobState::Completed as i32)));
    job_group.add(IppAttribute::new(
        "job-state-reasons",
        IppValue::Keyword("job-completed-successfully".to_string()),
    ));

    IppResponseBuilder::new(IppStatusCode::SuccessfulOk, request.request_id)
        .add_group(op)
        .add_group(job_group)
        .build()
}

fn handle_get_job_attributes(
    request: &IppRequest,
    printer: &Arc<PrinterState>,
) -> IppResponse {
    let job_id = request.get_operation_attributes()
        .and_then(|g| g.get("job-id"))
        .and_then(|v| match v {
            IppValue::Integer(i) => Some(*i as u32),
            _ => None,
        });

    let job_id = match job_id {
        Some(id) => id,
        None => return error_response(request.request_id, IppStatusCode::ClientErrorBadRequest, "Missing job-id"),
    };

    let job = match printer.active_jobs.get(&job_id) {
        Some(j) => j,
        None => return error_response(request.request_id, IppStatusCode::ClientErrorNotFound, "Job not found"),
    };

    let mut op = standard_operation_attrs(request.request_id);
    op.add(IppAttribute::new("status-message", IppValue::TextWithoutLanguage("successful-ok".to_string())));

    let mut job_group = IppAttributeGroup::new(DelimiterTag::JobAttributes);
    job_group.add(IppAttribute::new("job-id", IppValue::Integer(job.id as i32)));
    job_group.add(IppAttribute::new(
        "job-uri",
        IppValue::Uri(format!("{}/jobs/{}", printer.printer_uri, job.id)),
    ));
    job_group.add(IppAttribute::new("job-state", IppValue::Enum(job.state.clone() as i32)));
    job_group.add(IppAttribute::new(
        "job-state-reasons",
        IppValue::Keyword("job-completed-successfully".to_string()),
    ));
    job_group.add(IppAttribute::new(
        "job-name",
        IppValue::NameWithoutLanguage(job.name.clone()),
    ));
    job_group.add(IppAttribute::new(
        "job-originating-user-name",
        IppValue::NameWithoutLanguage(job.originating_user.clone()),
    ));
    job_group.add(IppAttribute::new(
        "job-k-octets",
        IppValue::Integer((job.size_bytes / 1024).max(1) as i32),
    ));
    job_group.add(IppAttribute::new(
        "time-at-creation",
        IppValue::Integer(job.time_created as i32),
    ));

    IppResponseBuilder::new(IppStatusCode::SuccessfulOk, request.request_id)
        .add_group(op)
        .add_group(job_group)
        .build()
}

fn handle_validate_job(request: &IppRequest) -> IppResponse {
    let mut op = standard_operation_attrs(request.request_id);
    op.add(IppAttribute::new(
        "status-message",
        IppValue::TextWithoutLanguage("successful-ok".to_string()),
    ));
    IppResponseBuilder::new(IppStatusCode::SuccessfulOk, request.request_id)
        .add_group(op)
        .build()
}

fn get_available_space(path: &std::path::Path) -> Option<u64> {
    // Use statvfs on Unix/Android
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let path_str = path.to_str()?;
        let path_cstr = CString::new(path_str).ok()?;
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(path_cstr.as_ptr(), &mut stat) == 0 {
                let avail = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
                return Some(avail);
            }
        }
    }
    #[allow(unused_variables)]
    let _ = path;
    None
}

/// Build a media-col collection for A4 (210×297 mm, dimensions in 1/100 mm units).
fn media_col_a4() -> IppValue {
    IppValue::Collection(vec![
        ("media-size".to_string(), IppValue::Collection(vec![
            ("x-dimension".to_string(), IppValue::Integer(21000)),
            ("y-dimension".to_string(), IppValue::Integer(29700)),
        ])),
        ("media-bottom-margin".to_string(), IppValue::Integer(0)),
        ("media-left-margin".to_string(),  IppValue::Integer(0)),
        ("media-right-margin".to_string(), IppValue::Integer(0)),
        ("media-top-margin".to_string(),   IppValue::Integer(0)),
        ("media-type".to_string(),         IppValue::Keyword("stationery".to_string())),
    ])
}

/// Build a media-col collection for US Letter (215.9×279.4 mm).
fn media_col_letter() -> IppValue {
    IppValue::Collection(vec![
        ("media-size".to_string(), IppValue::Collection(vec![
            ("x-dimension".to_string(), IppValue::Integer(21590)),
            ("y-dimension".to_string(), IppValue::Integer(27940)),
        ])),
        ("media-bottom-margin".to_string(), IppValue::Integer(0)),
        ("media-left-margin".to_string(),  IppValue::Integer(0)),
        ("media-right-margin".to_string(), IppValue::Integer(0)),
        ("media-top-margin".to_string(),   IppValue::Integer(0)),
        ("media-type".to_string(),         IppValue::Keyword("stationery".to_string())),
    ])
}

fn error_response(request_id: u32, status: IppStatusCode, message: &str) -> IppResponse {
    let mut op = standard_operation_attrs(request_id);
    op.add(IppAttribute::new(
        "status-message",
        IppValue::TextWithoutLanguage(message.to_string()),
    ));
    IppResponseBuilder::new(status, request_id)
        .add_group(op)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipp::parser::parse_ipp_request;
    use std::sync::atomic::AtomicU32;

    fn make_printer(dir: &std::path::Path) -> Arc<PrinterState> {
        Arc::new(PrinterState {
            printer_name: "InkPrint".to_string(),
            printer_uri: "ipp://127.0.0.1:631/ipp/print".to_string(),
            storage_dir: dir.to_path_buf(),
            job_counter: AtomicU32::new(1),
            active_jobs: dashmap::DashMap::new(),
        })
    }

    fn build_request(op: u16, request_id: u32, op_attrs: Vec<(u8, &str, &[u8])>, doc_data: &[u8]) -> Vec<u8> {
        let mut buf = vec![];
        buf.push(1); buf.push(1);
        buf.extend_from_slice(&op.to_be_bytes());
        buf.extend_from_slice(&request_id.to_be_bytes());
        buf.push(0x01); // operation-attributes group
        // Always add charset + language
        buf.push(0x47); // charset
        let n = b"attributes-charset"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
        let v = b"utf-8"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
        buf.push(0x48); // natural-language
        let n = b"attributes-natural-language"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
        let v = b"en"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);

        for (tag, name, value) in op_attrs {
            buf.push(tag);
            buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
            buf.extend_from_slice(value);
        }
        buf.push(0x03); // end-of-attributes
        buf.extend_from_slice(doc_data);
        buf
    }

    #[test]
    fn test_get_printer_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let printer = make_printer(dir.path());

        let raw = build_request(0x000B, 1, vec![
            (0x45, "printer-uri", b"ipp://127.0.0.1:631/ipp/print"),
        ], b"");
        let req = parse_ipp_request(&raw).unwrap();
        let resp_bytes = dispatch(&req, &printer, None);
        let resp = parse_ipp_request(&resp_bytes).unwrap();

        // Status OK
        assert_eq!(u16::from(resp.operation_id), 0x0000u16);

        let printer_group = resp.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::PrinterAttributes)
            .unwrap();

        assert!(matches!(printer_group.get("printer-name"), Some(IppValue::NameWithoutLanguage(_))));
        assert_eq!(printer_group.get("printer-state"), Some(&IppValue::Enum(3))); // idle
        assert_eq!(printer_group.get("printer-is-accepting-jobs"), Some(&IppValue::Boolean(true)));
    }

    #[test]
    fn test_get_printer_attributes_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let printer = make_printer(dir.path());

        let raw = build_request(0x000B, 2, vec![
            (0x45, "printer-uri", b"ipp://127.0.0.1:631/ipp/print"),
            (0x44, "requested-attributes", b"printer-name"),
        ], b"");
        let req = parse_ipp_request(&raw).unwrap();
        let resp_bytes = dispatch(&req, &printer, None);
        let resp = parse_ipp_request(&resp_bytes).unwrap();

        let printer_group = resp.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::PrinterAttributes)
            .unwrap();

        // printer-name should be present
        assert!(printer_group.get("printer-name").is_some());
        // printer-state should NOT be present (not requested)
        assert!(printer_group.get("printer-state").is_none());
    }

    #[test]
    fn test_print_job_saves_file() {
        let dir = tempfile::tempdir().unwrap();
        let printer = make_printer(dir.path());

        let pdf_data = b"%PDF-1.4 test document data";
        let raw = build_request(0x0002, 3, vec![
            (0x45, "printer-uri", b"ipp://127.0.0.1:631/ipp/print"),
            (0x42, "job-name", b"Test Job"),
            (0x42, "requesting-user-name", b"testuser"),
            (0x49, "document-format", b"application/pdf"),
        ], pdf_data);

        let req = parse_ipp_request(&raw).unwrap();
        let resp_bytes = dispatch(&req, &printer, None);
        let resp = parse_ipp_request(&resp_bytes).unwrap();

        // Status OK
        assert_eq!(u16::from(resp.operation_id), 0x0000u16);

        let job_group = resp.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::JobAttributes)
            .unwrap();
        assert!(matches!(job_group.get("job-id"), Some(IppValue::Integer(_))));
        assert_eq!(job_group.get("job-state"), Some(&IppValue::Enum(9))); // completed

        // Verify file exists on disk
        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
        let file_content = std::fs::read(files[0].as_ref().unwrap().path()).unwrap();
        assert_eq!(file_content, pdf_data);
    }

    #[test]
    fn test_get_job_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let printer = make_printer(dir.path());

        // First print a job
        let pdf_data = b"%PDF-1.4 test";
        let raw = build_request(0x0002, 4, vec![
            (0x45, "printer-uri", b"ipp://127.0.0.1:631/ipp/print"),
            (0x42, "job-name", b"My Doc"),
            (0x42, "requesting-user-name", b"alice"),
            (0x49, "document-format", b"application/pdf"),
        ], pdf_data);
        let req = parse_ipp_request(&raw).unwrap();
        let resp_bytes = dispatch(&req, &printer, None);
        let resp = parse_ipp_request(&resp_bytes).unwrap();
        let job_id_val = resp.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::JobAttributes)
            .and_then(|g| g.get("job-id"))
            .cloned()
            .unwrap();
        let job_id = if let IppValue::Integer(id) = job_id_val { id } else { panic!() };

        // Now get job attributes
        let job_id_bytes = (job_id as i32).to_be_bytes();
        let raw2 = build_request(0x0009, 5, vec![
            (0x45, "printer-uri", b"ipp://127.0.0.1:631/ipp/print"),
            (0x21, "job-id", &job_id_bytes),
        ], b"");
        let req2 = parse_ipp_request(&raw2).unwrap();
        let resp2_bytes = dispatch(&req2, &printer, None);
        let resp2 = parse_ipp_request(&resp2_bytes).unwrap();

        assert_eq!(u16::from(resp2.operation_id), 0x0000u16);
        let jg = resp2.attribute_groups.iter()
            .find(|g| g.delimiter == DelimiterTag::JobAttributes)
            .unwrap();
        assert_eq!(jg.get("job-id"), Some(&IppValue::Integer(job_id)));
        assert_eq!(jg.get("job-name"), Some(&IppValue::NameWithoutLanguage("My Doc".to_string())));
    }
}
