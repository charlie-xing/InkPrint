use std::net::Ipv4Addr;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::sync::oneshot;

pub struct MdnsAdvertiser {
    printer_name: String,
    ip: Ipv4Addr,
    port: u16,
}

impl MdnsAdvertiser {
    pub fn new(printer_name: String, ip: Ipv4Addr, port: u16) -> Self {
        Self { printer_name, ip, port }
    }

    pub async fn start(
        self,
        shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let daemon = ServiceDaemon::new()?;

        // Register with _universal._sub._ipp._tcp.local. as the service type.
        // The mdns-sd library's split_sub_domain() parses this into:
        //   - base type:  _ipp._tcp.local.
        //   - subtype:    _universal._sub._ipp._tcp.local.
        // Both PTR records are broadcast in the same packet, so:
        //   - dns-sd -B _ipp._tcp       → finds InkPrint  (for Android / Linux clients)
        //   - dns-sd -B _ipp._tcp,_universal → finds InkPrint  (triggers macOS "AirPrint")
        // Registering three separate ServiceInfos with the same fullname would cause
        // each to overwrite the previous in my_services, so we use exactly one registration.
        let service_type = "_universal._sub._ipp._tcp.local.";
        let instance_name = format!("{}.{}", self.printer_name, "_ipp._tcp.local.");

        let mut properties = std::collections::HashMap::new();
        properties.insert("txtvers".to_string(),  "1".to_string());
        // pdl: must not contain application/octet-stream (PWG spec); include pwg-raster for
        // IPP Everywhere compliance — clients that can't send raster will fall back to PDF.
        properties.insert("pdl".to_string(),
            "application/pdf,image/urf,image/pwg-raster,image/jpeg".to_string());
        properties.insert("rp".to_string(),       "ipp/print".to_string());
        properties.insert("ty".to_string(),       "InkPrint Virtual Printer".to_string());
        properties.insert("adminurl".to_string(), format!("http://{}:{}/", self.ip, self.port));
        properties.insert("UUID".to_string(),     "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e".to_string());
        properties.insert("Color".to_string(),    "F".to_string());
        properties.insert("Duplex".to_string(),   "F".to_string());
        properties.insert("Fax".to_string(),      "F".to_string());
        properties.insert("Scan".to_string(),     "F".to_string());
        properties.insert("Copies".to_string(),   "F".to_string());
        properties.insert("PaperMax".to_string(), "legal-A4".to_string());
        properties.insert("note".to_string(),     "E-ink reader virtual printer".to_string());
        // URF: real capability string required for AirPrint auto-discovery;
        // matches urf-supported in Get-Printer-Attributes.
        properties.insert("URF".to_string(),      "CP1,W8,RS300".to_string());

        let host_name = format!("{}.local.", self.printer_name.to_lowercase().replace(' ', "-"));
        let ip_str = self.ip.to_string();

        let service_info = ServiceInfo::new(
            service_type,
            &self.printer_name,
            &host_name,
            ip_str.as_str(),
            self.port,
            Some(properties),
        )?;

        daemon.register(service_info)?;

        // Monitor daemon events so errors are visible in logcat via tracing.
        // ServiceDaemon logs errors internally, but on Android tracing has no subscriber.
        // We poll the monitor channel for a short window after registration to surface them.
        let monitor = daemon.monitor()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match monitor.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(event) => {
                    let msg = format!("{:?}", event);
                    // Write to stderr — appears in Android logcat as System.err
                    eprintln!("[inkprint-mdns] daemon event: {}", msg);
                    tracing::info!("mDNS daemon event: {}", msg);
                }
                Err(_) => {}
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
        }
        eprintln!("[inkprint-mdns] registered '{}' (_ipp._tcp + _universal._sub) on {}:{}", self.printer_name, self.ip, self.port);
        tracing::info!(
            "mDNS: registered '{}' (_ipp._tcp + _universal._sub) on {}:{}",
            self.printer_name, self.ip, self.port
        );

        // Wait for shutdown signal
        let _ = shutdown.await;

        tracing::info!("mDNS: unregistering service");
        daemon.unregister(&instance_name).ok();
        daemon.shutdown()?;

        Ok(())
    }
}
