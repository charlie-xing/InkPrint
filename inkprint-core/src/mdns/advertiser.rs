use std::collections::HashMap;
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
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let daemon = ServiceDaemon::new()?;

        // _universal._sub._ipp._tcp.local. parsed by split_sub_domain() into:
        //   base type:  _ipp._tcp.local.         → found by all IPP clients
        //   subtype:    _universal._sub._ipp._tcp.local. → macOS selects "AirPrint" automatically
        let service_type  = "_universal._sub._ipp._tcp.local.";
        let instance_name = format!("{}._ipp._tcp.local.", self.printer_name);
        let host_name     = format!("{}.local.", self.printer_name.to_lowercase().replace(' ', "-"));
        let ip_str        = self.ip.to_string();

        let register = |daemon: &ServiceDaemon| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut props = HashMap::new();
            props.insert("txtvers".to_string(), "1".to_string());
            props.insert("pdl".to_string(),
                "application/pdf,image/urf,image/pwg-raster,image/jpeg".to_string());
            props.insert("rp".to_string(),       "ipp/print".to_string());
            props.insert("ty".to_string(),       "InkPrint Virtual Printer".to_string());
            props.insert("adminurl".to_string(), format!("http://{}:{}/", ip_str, self.port));
            props.insert("UUID".to_string(),     "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e".to_string());
            props.insert("Color".to_string(),    "F".to_string());
            props.insert("Duplex".to_string(),   "F".to_string());
            props.insert("Fax".to_string(),      "F".to_string());
            props.insert("Scan".to_string(),     "F".to_string());
            props.insert("Copies".to_string(),   "F".to_string());
            props.insert("PaperMax".to_string(), "legal-A4".to_string());
            props.insert("note".to_string(),     "E-ink reader virtual printer".to_string());
            props.insert("URF".to_string(),      "CP1,W8,RS300".to_string());

            let info = ServiceInfo::new(
                service_type,
                &self.printer_name,
                &host_name,
                ip_str.as_str(),
                self.port,
                Some(props),
            )?;
            daemon.register(info)?;
            Ok(())
        };

        register(&daemon)?;
        log::info!("mDNS: registered '{}' on {}:{}", self.printer_name, self.ip, self.port);

        // Re-announce every 60 s so remote caches never expire between queries.
        // mdns-sd's SRV/A records have host_ttl = 120 s; clients send a refresh
        // query at ~96 s.  If Android's WiFi power-save drops that query, the
        // printer disappears.  Proactive re-registration guarantees 2 fresh
        // multicast announcements before any record can expire.
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        interval.tick().await; // skip the immediate first tick

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => break,
                _ = interval.tick() => {
                    daemon.unregister(&instance_name).ok();
                    if let Err(e) = register(&daemon) {
                        log::warn!("mDNS re-announce failed: {}", e);
                    } else {
                        log::debug!("mDNS: re-announced '{}'", self.printer_name);
                    }
                }
            }
        }

        log::info!("mDNS: unregistering '{}'", self.printer_name);
        daemon.unregister(&instance_name).ok();
        daemon.shutdown()?;

        Ok(())
    }
}
