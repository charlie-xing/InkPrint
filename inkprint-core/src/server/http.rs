use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::ipp::operations::{dispatch, PrintJobCallback};
use crate::ipp::parser::parse_ipp_request;
use crate::ipp::printer::PrinterState;

pub struct HttpServer {
    printer: Arc<PrinterState>,
    callback: Option<Arc<dyn PrintJobCallback>>,
}

impl HttpServer {
    pub fn new(printer: Arc<PrinterState>, callback: Option<Arc<dyn PrintJobCallback>>) -> Self {
        Self { printer, callback }
    }

    async fn handle_request(
        printer: Arc<PrinterState>,
        callback: Option<Arc<dyn PrintJobCallback>>,
        local_addr: SocketAddr,
        req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, hyper::Error> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let printer_uri = printer_uri_for(
            req.headers().get(hyper::header::HOST).and_then(|v| v.to_str().ok()),
            local_addr,
        );

        // Only accept POST /ipp/print
        if method != Method::POST || (path != "/ipp/print" && path != "/") {
            tracing::debug!("Rejected request: {} {}", method, path);
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap());
        }

        // Check Content-Type
        let content_type = req.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.starts_with("application/ipp") {
            tracing::debug!("Rejected: bad Content-Type: {}", content_type);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Expected application/ipp")))
                .unwrap());
        }

        // Read body (enforce 512MB max to prevent OOM on large PDFs)
        const MAX_BODY_BYTES: usize = 512 * 1024 * 1024;
        let body_bytes = match req.into_body().collect().await {
            Ok(collected) => {
                let b = collected.to_bytes();
                if b.len() > MAX_BODY_BYTES {
                    tracing::warn!("Request body too large: {} bytes", b.len());
                    return Ok(Response::builder()
                        .status(StatusCode::PAYLOAD_TOO_LARGE)
                        .body(Full::new(Bytes::from("Request body too large")))
                        .unwrap());
                }
                b
            }
            Err(e) => {
                tracing::error!("Failed to read request body: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from("Failed to read body")))
                    .unwrap());
            }
        };

        // Parse IPP
        let ipp_request = match parse_ipp_request(&body_bytes) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to parse IPP request: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Invalid IPP request")))
                    .unwrap());
            }
        };

        tracing::info!(
            "IPP {:?} request_id={} doc_bytes={}",
            ipp_request.operation_id,
            ipp_request.request_id,
            ipp_request.document_data.len()
        );

        // Dispatch IPP operation
        let cb_ref: Option<&dyn PrintJobCallback> = callback.as_deref();
        let response_bytes = dispatch(&ipp_request, &printer, &printer_uri, cb_ref);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/ipp")
            .body(Full::new(Bytes::from(response_bytes)))
            .unwrap())
    }

    /// Run with a pre-bound listener (preferred — bind errors are caught earlier)
    pub async fn run_with_listener(
        self,
        listener: TcpListener,
        shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!("IPP HTTP server listening on {:?}", listener.local_addr());
        self.accept_loop(listener, shutdown).await
    }

    pub async fn run(
        self,
        addr: SocketAddr,
        shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("IPP HTTP server listening on {}", addr);
        self.accept_loop(listener, shutdown).await
    }

    async fn accept_loop(
        self,
        listener: TcpListener,
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    tracing::info!("HTTP server shutting down");
                    break;
                }
                result = listener.accept() => {
                    let (stream, peer_addr) = match result {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                            continue;
                        }
                    };

                    tracing::debug!("Connection from {}", peer_addr);
                    // The address the client connected to — Wi-Fi, Tailscale, ... —
                    // used for the printer URI when the request has no usable Host.
                    let local_addr = match stream.local_addr() {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::error!("No local address for {}: {}", peer_addr, e);
                            continue;
                        }
                    };
                    let io = TokioIo::new(stream);
                    let printer = self.printer.clone();
                    let callback = self.callback.clone();

                    tokio::spawn(async move {
                        let svc = hyper::service::service_fn(move |req| {
                            let printer = printer.clone();
                            let callback = callback.clone();
                            async move {
                                HttpServer::handle_request(printer, callback, local_addr, req).await
                            }
                        });

                        if let Err(e) = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, svc)
                            .await
                        {
                            tracing::debug!("Connection error: {}", e);
                        }
                    });
                }
            }
        }

        Ok(())
    }
}

/// The printer URI as the client addressed it: taken from the `Host` header
/// (so an IP, a Tailscale MagicDNS name or `.local` name all round-trip),
/// else from the local address the connection arrived on. A Host without a
/// port gets the port we are actually listening on.
pub fn printer_uri_for(host: Option<&str>, local: SocketAddr) -> String {
    let authority = host
        .map(str::trim)
        .filter(|h| is_valid_authority(h))
        .map(|h| if has_port(h) { h.to_string() } else { format!("{}:{}", h, local.port()) })
        .unwrap_or_else(|| local.to_string());
    format!("ipp://{}/ipp/print", authority)
}

/// `host[:port]` or `[v6]:port` with nothing that could break out of a URI.
fn is_valid_authority(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 255
        && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

fn has_port(h: &str) -> bool {
    match h.rfind(']') {
        Some(end) => h[end..].contains(':'),   // [v6]:port
        None => h.contains(':'),               // host:port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> SocketAddr { "192.168.2.5:6310".parse().unwrap() }

    #[test]
    fn printer_uri_follows_host_header() {
        assert_eq!(printer_uri_for(Some("100.70.177.81:6310"), local()), "ipp://100.70.177.81:6310/ipp/print");
        assert_eq!(printer_uri_for(Some("xp.tail33a0b2.ts.net:6310"), local()), "ipp://xp.tail33a0b2.ts.net:6310/ipp/print");
        assert_eq!(printer_uri_for(Some("[fd7a::1]:6310"), local()), "ipp://[fd7a::1]:6310/ipp/print");
    }

    #[test]
    fn printer_uri_adds_missing_port() {
        assert_eq!(printer_uri_for(Some("inkprint.local"), local()), "ipp://inkprint.local:6310/ipp/print");
    }

    #[test]
    fn printer_uri_falls_back_to_local_address() {
        assert_eq!(printer_uri_for(None, local()), "ipp://192.168.2.5:6310/ipp/print");
        assert_eq!(printer_uri_for(Some(""), local()), "ipp://192.168.2.5:6310/ipp/print");
        assert_eq!(printer_uri_for(Some("evil/path?x"), local()), "ipp://192.168.2.5:6310/ipp/print");
    }
}
