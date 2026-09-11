use super::command;
use crate::Result;
use std::{
    net::Ipv4Addr,
    process::Output,
    time::{Duration, Instant},
};

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
        Self::query(&["-n", "get", "default"])
    }
    fn query(args: &[&str]) -> Result<Option<Self>> {
        let result = command::run("/sbin/route", args)?;
        Reply::parse(&result)
    }
    pub fn vpn_present() -> Result<bool> {
        let table = command::text("/usr/sbin/netstat", &["-rn", "-f", "inet"])?;
        Table::has_vpn(&table)
    }
    pub fn apply(&self) -> Result<()> {
        let current = Self::read()?;
        if current.as_ref().is_some_and(Self::is_vpn) || Self::vpn_present()? {
            return Err("VPN default detected; refusing to override it".into());
        }
        self.write(current.as_ref())
    }
    fn write(&self, current: Option<&Self>) -> Result<()> {
        let gateway = self.gateway.ok_or("cannot apply a non-IPv4 gateway")?;
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
            || address.is_loopback()
            || address.is_multicast()
            || address.is_broadcast()
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
}

/// Classifies route(8) output independently of its unreliable exit status.
pub struct Reply;
impl Reply {
    pub fn parse(output: &Output) -> Result<Option<Snapshot>> {
        let stdout = std::str::from_utf8(&output.stdout)?.trim();
        let stderr = std::str::from_utf8(&output.stderr)?.trim();
        // Darwin can exit successfully when RTM_GET reports ESRCH. This exact
        // empty reply means that a default may safely be added. Contradictory
        // output and all other diagnostics remain inspection failures.
        if stdout.is_empty() && stderr == "route: writing to routing socket: not in table" {
            return Ok(None);
        }
        if !output.status.success() || !stderr.is_empty() {
            return Err("cannot inspect default route; refusing routing changes".into());
        }
        Snapshot::parse(stdout)
            .map(Some)
            .ok_or_else(|| "unrecognized default route; refusing routing changes".into())
    }
}

/// Fallback routing after the owned DHCP service and interface are gone.
pub struct Recovery;
impl Recovery {
    pub fn permits(current: Option<&Snapshot>, owned: &str) -> bool {
        current.is_none_or(|route| route.interface == owned && !route.is_vpn())
    }

    pub fn select(
        previous: Option<&Snapshot>,
        fresh: Option<Snapshot>,
        active: bool,
    ) -> Option<Snapshot> {
        let previous = previous?;
        if !active || previous.interface.starts_with("feth") || previous.is_vpn() {
            return None;
        }
        fresh.filter(|route| {
            route.interface == previous.interface
                && route.gateway.is_some_and(|gateway| {
                    !gateway.is_unspecified()
                        && !gateway.is_loopback()
                        && !gateway.is_multicast()
                        && !gateway.is_broadcast()
                })
        })
    }

    pub fn restore(previous: Option<&Snapshot>, owned: &str) -> Result<()> {
        let Some(previous) =
            previous.filter(|route| !route.interface.starts_with("feth") && !route.is_vpn())
        else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut applied = None;
        let mut confirmations = 0;
        loop {
            let current = Snapshot::read()?;
            if Snapshot::vpn_present()? {
                return Ok(());
            }
            if !Self::permits(current.as_ref(), owned) {
                // An independently selected default always wins. If it is the
                // route we just wrote, verify it persists across configd's
                // asynchronous processing of the removed DHCP service.
                if applied
                    .as_ref()
                    .is_some_and(|route| current.as_ref() == Some(route))
                {
                    confirmations += 1;
                    if confirmations >= 2 {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            } else {
                confirmations = 0;
                let active = command::text("/sbin/ifconfig", &[&previous.interface])
                    .is_ok_and(|text| text.lines().any(|line| line.trim() == "status: active"));
                let address =
                    command::text("/usr/sbin/ipconfig", &["getifaddr", &previous.interface])
                        .ok()
                        .and_then(|text| text.parse::<Ipv4Addr>().ok())
                        .is_some_and(|address| {
                            !address.is_unspecified()
                                && !address.is_loopback()
                                && !address.is_multicast()
                                && !address.is_broadcast()
                        });
                if active && address {
                    // Prefer the current DHCP router, which can differ from
                    // the gateway saved before a long-running USB session.
                    // A static service may instead retain a scoped default.
                    // Never replay a stale cached gateway as the fallback.
                    let fresh = match Snapshot::dhcp(&previous.interface) {
                        Some(route) => Some(route),
                        None => Snapshot::query(&[
                            "-n",
                            "get",
                            "-ifscope",
                            &previous.interface,
                            "default",
                        ])?,
                    };
                    if let Some(target) = Self::select(Some(previous), fresh, true) {
                        let current = Snapshot::read()?;
                        if !Self::permits(current.as_ref(), owned) || Snapshot::vpn_present()? {
                            return Ok(());
                        }
                        target.write(current.as_ref())?;
                        applied = Some(target);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(
                    "no stable active fallback route appeared within the recovery deadline".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(250));
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
