#![cfg(target_os = "macos")]
use galaxybridge::macos::{bpf::Frame, worker_app::Entitlements};

#[test]
fn root_ethernet_boundary_allows_only_ipv4_and_arp() {
    let mut bytes = [0u8; 1514];
    for kind in 0..=u16::MAX {
        bytes[12..14].copy_from_slice(&kind.to_be_bytes());
        assert_eq!(Frame::permits(&bytes), matches!(kind, 0x0800 | 0x0806));
    }
    bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    for len in 0..14 {
        assert!(!Frame::permits(&bytes[..len]));
    }
    let mut oversized = bytes.to_vec();
    oversized.push(0);
    assert!(!Frame::permits(&oversized));
}

#[test]
fn worker_entitlements_require_exact_sandbox_and_usb_permissions() {
    let sandbox = "<key>com.apple.security.app-sandbox</key><true/>";
    let usb = "<key>com.apple.security.device.usb</key><true/>";
    let valid = format!("<plist version=\"1.0\"><dict>{sandbox}{usb}</dict></plist>");
    Entitlements::check(&valid).unwrap();
    Entitlements::check(&format!(
        "<plist version=\"1.0\"><dict>{usb}{sandbox}</dict></plist>"
    ))
    .unwrap();
    let declaration = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"https://www.apple.com/DTDs/PropertyList-1.0.dtd\">";
    Entitlements::check(&format!("{declaration}{valid}")).unwrap();
    for altered in [
        valid.replace(sandbox, ""),
        valid.replace("<true/>", "<false/>"),
        valid.replace(usb, sandbox),
        valid.replace(
            "</dict>",
            "<key>com.apple.security.network.client</key><true/></dict>",
        ),
        valid.replace(
            "</dict>",
            "<key>com.apple.security.inherit</key><true/></dict>",
        ),
        valid.replace(
            "</dict>",
            "<key>com.apple.security.get-task-allow</key><false/></dict>",
        ),
        valid.replace(
            "com.apple.security.device.usb",
            "com.apple.security.device.&#117;sb",
        ),
        format!("<!-- ignored? -->{valid}"),
        format!("{valid}<plist version=\"1.0\"><dict/></plist>"),
    ] {
        assert!(Entitlements::check(&altered).is_err(), "{altered}");
    }
}
