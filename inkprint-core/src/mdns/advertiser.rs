use std::collections::HashMap;
use std::net::Ipv4Addr;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::sync::oneshot;

use crate::ipp::printer::PrinterProfile;

pub struct MdnsAdvertiser {
    profiles: Vec<PrinterProfile>,
    host_name: String,
    ip: Ipv4Addr,
    port: u16,
}

impl MdnsAdvertiser {
    /// Advertises every profile (PDF printer, EPUB printer) as its own IPP
    /// service on one shared host record.
    pub fn new(profiles: Vec<PrinterProfile>, base_name: &str, ip: Ipv4Addr, port: u16) -> Self {
        let host_name = format!("{}.local.", base_name.to_lowercase().replace(' ', "-"));
        Self { profiles, host_name, ip, port }
    }

    fn register(&self, daemon: &ServiceDaemon, profile: &PrinterProfile)
        -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        // _universal._sub._ipp._tcp.local. parsed by split_sub_domain() into:
        //   base type:  _ipp._tcp.local.         → found by all IPP clients
        //   subtype:    _universal._sub._ipp._tcp.local. → macOS selects "AirPrint" automatically
        let service_type = "_universal._sub._ipp._tcp.local.";
        let ip_str = self.ip.to_string();

        let mut props = HashMap::new();
        props.insert("txtvers".to_string(), "1".to_string());
        props.insert("pdl".to_string(),
            crate::ipp::operations::SUPPORTED_DOCUMENT_FORMATS.join(","));
        props.insert("rp".to_string(),       profile.resource.to_string());
        props.insert("ty".to_string(),       profile.make_and_model.to_string());
        props.insert("adminurl".to_string(), format!("http://{}:{}/", ip_str, self.port));
        props.insert("UUID".to_string(),     profile.uuid.to_string());
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
            &profile.name,
            &self.host_name,
            ip_str.as_str(),
            self.port,
            Some(props),
        )?;
        daemon.register(info)?;
        Ok(())
    }

    fn instance_name(profile: &PrinterProfile) -> String {
        format!("{}._ipp._tcp.local.", profile.name)
    }

    pub async fn start(
        self,
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let daemon = ServiceDaemon::new()?;

        for p in &self.profiles {
            self.register(&daemon, p)?;
            log::info!("mDNS: registered '{}' on {}:{}/{}", p.name, self.ip, self.port, p.resource);
        }

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
                    for p in &self.profiles {
                        daemon.unregister(&Self::instance_name(p)).ok();
                        if let Err(e) = self.register(&daemon, p) {
                            log::warn!("mDNS re-announce of '{}' failed: {}", p.name, e);
                        } else {
                            log::debug!("mDNS: re-announced '{}'", p.name);
                        }
                    }
                }
            }
        }

        for p in &self.profiles {
            log::info!("mDNS: unregistering '{}'", p.name);
            daemon.unregister(&Self::instance_name(p)).ok();
        }
        daemon.shutdown()?;

        Ok(())
    }
}
