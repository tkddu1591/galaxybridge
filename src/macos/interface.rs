use super::command;
use crate::Result;

pub struct Pair {
    pub system: String,
    pub transport: String,
}
impl Pair {
    pub fn create() -> Result<Self> {
        for key in ["net.link.fake.hwcsum", "net.link.fake.lro"] {
            if command::text("/usr/sbin/sysctl", &["-n", key])? != "0" {
                return Err(format!(
                    "{key} must be 0; refusing to change global kernel settings automatically"
                )
                .into());
            }
        }
        let system = command::text("/sbin/ifconfig", &["feth", "create"])?;
        if !Self::valid_name(&system) {
            return Err("unexpected interface name".into());
        }
        let mut pair = Self {
            system,
            transport: String::new(),
        };
        pair.transport = command::text("/sbin/ifconfig", &["feth", "create"])?;
        if !Self::valid_name(&pair.transport) {
            pair.transport.clear();
            return Err("unexpected interface name".into());
        }
        for iface in [&pair.system, &pair.transport] {
            command::text("/sbin/ifconfig", &[iface, "mtu", "1500"])?;
        }
        command::text("/sbin/ifconfig", &[&pair.transport, "up"])?;
        Ok(pair)
    }
    pub fn valid_name(name: &str) -> bool {
        name.strip_prefix("feth")
            .is_some_and(|n| !n.is_empty() && n.len() <= 5 && n.bytes().all(|c| c.is_ascii_digit()))
    }
    pub fn activate(&self, mac: [u8; 6]) -> Result<()> {
        let text = mac
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        command::text("/sbin/ifconfig", &[&self.system, "ether", &text])?;
        // IPConfiguration caches the hardware address on the active-link event.
        // Pair only after the final MAC has been set, avoiding stale DHCP IDs.
        command::text("/sbin/ifconfig", &[&self.system, "peer", &self.transport])?;
        command::text("/sbin/ifconfig", &[&self.system, "up"])?;
        Ok(())
    }
    pub fn dhcp(&self) -> Result<()> {
        command::text("/usr/sbin/ipconfig", &["set", &self.system, "DHCP"])?;
        Ok(())
    }

    pub fn remove(&mut self) -> Result<()> {
        // Clear ownership before running commands so Drop cannot later remove
        // an interface whose name has been reused after this teardown.
        let system = std::mem::take(&mut self.system);
        let transport = std::mem::take(&mut self.transport);
        let mut failure = None;
        if Self::valid_name(&system) {
            // Withdraw IPConfiguration's temporary address, routes and resolver
            // state before destroying the device that owns that DHCP service.
            if let Err(error) = command::text("/usr/sbin/ipconfig", &["set", &system, "NONE"]) {
                failure = Some(error);
            }
        }
        for iface in [&transport, &system] {
            if Self::valid_name(iface) {
                if let Err(error) = command::text("/sbin/ifconfig", &[iface, "destroy"]) {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
impl Drop for Pair {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            eprintln!("GalaxyBridge: interface cleanup: {error}");
        }
    }
}
