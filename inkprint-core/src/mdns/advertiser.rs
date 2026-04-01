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

        let service_type   = "_ipp._tcp.local.";
        // IPP Everywhere identity (PWG 5100.14 §4.2.1)
        let ipp_everywhere = "_print._sub._ipp._tcp.local.";
        // AirPrint / macOS auto-discovery
        let airprint_type  = "_universal._sub._ipp._tcp.local.";

        let instance_name      = format!("{}.{}", self.printer_name, service_type);
        let ipp_everywhere_inst = format!("{}.{}", self.printer_name, ipp_everywhere);
        let airprint_instance  = format!("{}.{}", self.printer_name, airprint_type);

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

        let host_name = format!("{}.", self.ip);
        let ip_str = self.ip.to_string();

        let service_info = ServiceInfo::new(
            service_type,
            &self.printer_name,
            &host_name,
            ip_str.as_str(),
            self.port,
            Some(properties.clone()),
        )?;

        // IPP Everywhere subtype (PWG spec)
        let ipp_everywhere_info = ServiceInfo::new(
            ipp_everywhere,
            &self.printer_name,
            &host_name,
            ip_str.as_str(),
            self.port,
            Some(properties.clone()),
        )?;

        // AirPrint subtype for macOS/iOS auto-discovery
        let airprint_info = ServiceInfo::new(
            airprint_type,
            &self.printer_name,
            &host_name,
            ip_str.as_str(),
            self.port,
            Some(properties),
        )?;

        daemon.register(service_info)?;
        daemon.register(ipp_everywhere_info)?;
        daemon.register(airprint_info)?;
        tracing::info!(
            "mDNS: registered '{}' (_ipp._tcp + _print._sub + _universal._sub) on {}:{}",
            self.printer_name, self.ip, self.port
        );

        // Wait for shutdown signal
        let _ = shutdown.await;

        tracing::info!("mDNS: unregistering service");
        daemon.unregister(&instance_name).ok();
        daemon.unregister(&ipp_everywhere_inst).ok();
        daemon.unregister(&airprint_instance).ok();
        daemon.shutdown()?;

        Ok(())
    }
}
