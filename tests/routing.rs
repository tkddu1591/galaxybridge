#![cfg(target_os = "macos")]
use galaxybridge::macos::route::{Reply, Snapshot, Table};
use std::{os::unix::process::ExitStatusExt, process::Output};

#[test]
fn missing_route_is_recognized_even_when_darwin_exits_successfully() {
    for status in [0, 1 << 8] {
        let output = Output {
            status: std::process::ExitStatus::from_raw(status),
            stdout: Vec::new(),
            stderr: b"route: writing to routing socket: not in table\n".to_vec(),
        };
        assert!(Reply::parse(&output).unwrap().is_none());
    }
}

#[test]
fn route_reply_preserves_valid_gateway_and_vpn_identity() {
    for text in [
        "gateway: 192.0.2.1\ninterface: en0\n",
        "gateway: link#31\ninterface: utun7\n",
    ] {
        let output = Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(Reply::parse(&output).unwrap(), Snapshot::parse(text));
    }
}

#[test]
fn unknown_or_contradictory_route_replies_still_fail_closed() {
    let valid = "gateway: 192.0.2.1\ninterface: en0";
    let missing = "route: writing to routing socket: not in table";
    for (status, stdout, stderr) in [
        (0, "", ""),
        (0, "", "route: permission denied"),
        (1 << 8, valid, ""),
        (0, valid, missing),
        (0, "", "unrecognized message: not in table"),
        (
            0,
            "",
            "route: writing to routing socket: not in table\nother error",
        ),
        (0, "unrecognized route data", ""),
    ] {
        let output = Output {
            status: std::process::ExitStatus::from_raw(status),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        };
        assert!(
            Reply::parse(&output).is_err(),
            "{status}: {stdout:?} {stderr:?}"
        );
    }
}

#[test]
#[ignore = "requires a macOS host without a scoped loopback default route; read-only"]
fn darwin_missing_route_reply_matches_the_observed_system_behavior() {
    let output = std::process::Command::new("/sbin/route")
        .args(["-n", "get", "-ifscope", "lo0", "default"])
        .env_clear()
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        std::str::from_utf8(&output.stderr).unwrap().trim(),
        "route: writing to routing socket: not in table"
    );
    assert!(Reply::parse(&output).unwrap().is_none());
}

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
