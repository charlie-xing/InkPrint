/// Integration test: start the IPP server, send a Print-Job via HTTP, verify the file is saved.
///
/// Uses a random port to avoid conflicts.

use std::time::Duration;
use tokio::time::sleep;

fn build_print_job_request(request_id: u32, printer_uri: &str, doc_data: &[u8]) -> Vec<u8> {
    let mut buf = vec![];
    // version 1.1
    buf.push(1); buf.push(1);
    // Print-Job = 0x0002
    buf.extend_from_slice(&0x0002u16.to_be_bytes());
    buf.extend_from_slice(&request_id.to_be_bytes());

    // operation-attributes group
    buf.push(0x01);
    // attributes-charset
    buf.push(0x47);
    let n = b"attributes-charset"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"utf-8"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    // attributes-natural-language
    buf.push(0x48);
    let n = b"attributes-natural-language"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"en"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    // printer-uri
    buf.push(0x45);
    let n = b"printer-uri"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    buf.extend_from_slice(&(printer_uri.len() as u16).to_be_bytes()); buf.extend_from_slice(printer_uri.as_bytes());
    // job-name
    buf.push(0x42);
    let n = b"job-name"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"Integration Test Doc"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    // requesting-user-name
    buf.push(0x42);
    let n = b"requesting-user-name"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"testuser"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    // document-format
    buf.push(0x49);
    let n = b"document-format"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"application/pdf"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);

    // end-of-attributes
    buf.push(0x03);

    // document data
    buf.extend_from_slice(doc_data);

    buf
}

#[tokio::test]
async fn test_print_job_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let port = 16310u16; // Use non-standard port for testing

    let config = inkprint_core::server::listener::ServerConfig {
        port,
        storage_dir: dir.path().to_path_buf(),
        printer_name: "TestPrinter".to_string(),
        callback: None,
    };

    let handle = inkprint_core::server::listener::start(config).await
        .expect("Server should start");

    // Give server a moment to be ready
    sleep(Duration::from_millis(100)).await;

    let printer_uri = format!("ipp://127.0.0.1:{}/ipp/print", port);
    let fake_pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n%%EOF";

    let ipp_body = build_print_job_request(1, &printer_uri, fake_pdf);

    // Send HTTP POST with application/ipp
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/ipp/print", port);
    let resp = client
        .post(&url)
        .header("Content-Type", "application/ipp")
        .body(ipp_body)
        .send()
        .await
        .expect("HTTP request should succeed");

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/ipp")
    );

    let resp_bytes = resp.bytes().await.unwrap();

    // Parse the IPP response
    let ipp_resp = inkprint_core::ipp::parser::parse_ipp_request(&resp_bytes)
        .expect("Response should be valid IPP");

    // Check status: 0x0000 = Successful-Ok
    assert_eq!(u16::from(ipp_resp.operation_id), 0x0000u16, "Expected Successful-Ok");

    // Verify file was saved
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1, "Expected exactly one file saved");

    let saved = std::fs::read(files[0].path()).unwrap();
    assert_eq!(saved, fake_pdf, "Saved file should match sent document");

    // Stop server
    handle.stop();
}

#[tokio::test]
async fn test_get_printer_attributes_http() {
    let dir = tempfile::tempdir().unwrap();
    let port = 16311u16;

    let config = inkprint_core::server::listener::ServerConfig {
        port,
        storage_dir: dir.path().to_path_buf(),
        printer_name: "TestPrinter2".to_string(),
        callback: None,
    };

    let handle = inkprint_core::server::listener::start(config).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    let printer_uri = format!("ipp://127.0.0.1:{}/ipp/print", port);
    let mut buf = vec![];
    buf.push(1); buf.push(1);
    buf.extend_from_slice(&0x000Bu16.to_be_bytes()); // GetPrinterAttributes
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.push(0x01);
    buf.push(0x47);
    let n = b"attributes-charset"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"utf-8"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    buf.push(0x48);
    let n = b"attributes-natural-language"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    let v = b"en"; buf.extend_from_slice(&(v.len() as u16).to_be_bytes()); buf.extend_from_slice(v);
    buf.push(0x45);
    let n = b"printer-uri"; buf.extend_from_slice(&(n.len() as u16).to_be_bytes()); buf.extend_from_slice(n);
    buf.extend_from_slice(&(printer_uri.len() as u16).to_be_bytes());
    buf.extend_from_slice(printer_uri.as_bytes());
    buf.push(0x03);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/ipp/print", port))
        .header("Content-Type", "application/ipp")
        .body(buf)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let resp_bytes = resp.bytes().await.unwrap();
    let ipp_resp = inkprint_core::ipp::parser::parse_ipp_request(&resp_bytes).unwrap();
    assert_eq!(u16::from(ipp_resp.operation_id), 0x0000u16);

    let printer_group = ipp_resp.attribute_groups.iter()
        .find(|g| g.delimiter == inkprint_core::ipp::types::DelimiterTag::PrinterAttributes)
        .expect("Should have printer attributes group");
    assert!(printer_group.get("printer-name").is_some());

    handle.stop();
}
