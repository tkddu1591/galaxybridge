#![cfg(target_os = "macos")]
use galaxybridge::macos::route::{Snapshot, Table};

#[test]
fn link_gateway_retains_vpn_identity() {
    let route = Snapshot::parse("gateway: link#31\ninterface: utun7").unwrap();
    assert!(route.is_vpn());
    assert!(route.gateway.is_none());
    assert!(Snapshot::parse("gateway: unexpected\ninterface: en0").is_none());
    assert!(Snapshot::parse("interface: en0").is_none());
}

#[test]
fn vpn_detection_covers_split_defaults_and_only_the_netif_column() {
    let header = "Routing tables\nInternet:\nDestination Gateway Flags Netif Expire\n";
    for row in [
        "default 10.0.0.1 UG utun3",
        "0/1 link#21 U utun0",
        "128.0/1 link#21 U utun0",
        "10.2/16 link#31 U utun9",
    ] {
        assert!(Table::has_vpn(&format!("{header}{row}")).unwrap());
    }
    assert!(
        !Table::has_vpn(&format!(
            "{header}default 192.0.2.1 UG en0\nutun3 192.0.2.1 U en0"
        ))
        .unwrap()
    );
    assert!(Table::has_vpn("unknown table layout").is_err());
}
