use super::command;
use crate::Result;
use std::net::Ipv4Addr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub interface: String,
    pub gateway: Option<Ipv4Addr>,
}
impl Snapshot {
    pub fn parse(text: &str) -> Option<Self> {
        let interface = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("interface:"))?
            .trim();
        if interface.is_empty()
            || interface.len() > 15
            || !interface.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        let raw_gateway = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("gateway:"))?
            .trim();
        let gateway = raw_gateway.parse().ok();
        if gateway.is_none()
            && !raw_gateway
                .strip_prefix("link#")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        Some(Self {
            interface: interface.into(),
            gateway,
        })
    }
    pub fn read() -> Result<Option<Self>> {
        let result = command::run("/sbin/route", &["-n", "get", "default"])?;
        if !result.status.success() {
            if String::from_utf8_lossy(&result.stderr).contains("not in table") {
                return Ok(None);
            }
            return Err("cannot inspect default route; refusing routing changes".into());
        }
        Self::parse(&String::from_utf8(result.stdout)?)
            .map(Some)
            .ok_or_else(|| "unrecognized default route; refusing routing changes".into())
    }
    pub fn vpn_present() -> Result<bool> {
        let table = command::text("/usr/sbin/netstat", &["-rn", "-f", "inet"])?;
        Table::has_vpn(&table)
    }
    pub fn apply(&self) -> Result<()> {
        let gateway = self.gateway.ok_or("cannot apply a non-IPv4 gateway")?;
        let current = Self::read()?;
        if current.as_ref().is_some_and(Self::is_vpn) || Self::vpn_present()? {
            return Err("VPN default detected; refusing to override it".into());
        }
        let action = if current.is_some() { "change" } else { "add" };
        command::text(
            "/sbin/route",
            &[
                "-n",
                action,
                "default",
                &gateway.to_string(),
                "-ifp",
                &self.interface,
            ],
        )?;
        if Self::read()?.as_ref() != Some(self) {
            return Err("default route did not select the requested interface".into());
        }
        Ok(())
    }
    pub fn dhcp(interface: &str) -> Option<Self> {
        let address: Ipv4Addr = command::text("/usr/sbin/ipconfig", &["getifaddr", interface])
            .ok()?
            .parse()
            .ok()?;
        let gateway: Ipv4Addr =
            command::text("/usr/sbin/ipconfig", &["getoption", interface, "router"])
                .ok()?
                .parse()
                .ok()?;
        if address.is_unspecified()
            || gateway.is_unspecified()
            || gateway.is_loopback()
            || gateway.is_multicast()
            || gateway.is_broadcast()
        {
            return None;
        }
        Some(Self {
            interface: interface.into(),
            gateway: Some(gateway),
        })
    }
    pub fn is_vpn(&self) -> bool {
        self.interface.starts_with("utun")
    }
    pub fn restore(previous: &Option<Self>, owned: &str) {
        let Ok(current) = Self::read() else {
            return;
        };
        if current.as_ref().is_some_and(|s| s.interface != owned) {
            return;
        }
        if let Some(previous) = previous {
            if !previous.interface.starts_with("feth")
                && command::text("/usr/sbin/ipconfig", &["getifaddr", &previous.interface]).is_ok()
            {
                let _ = previous.apply();
            }
        }
    }
}

pub struct Table;
impl Table {
    pub fn has_vpn(text: &str) -> Result<bool> {
        let mut interface_column = None;
        for line in text.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.first() == Some(&"Destination") {
                interface_column = fields.iter().position(|v| *v == "Netif");
                continue;
            }
            if let Some(index) = interface_column {
                if fields.get(index).is_some_and(|v| {
                    v.strip_prefix("utun")
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                }) {
                    return Ok(true);
                }
            }
        }
        if interface_column.is_none() {
            return Err("unrecognized route table; refusing routing changes".into());
        }
        Ok(false)
    }
}
