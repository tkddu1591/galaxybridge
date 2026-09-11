use galaxybridge::usb::devices::{Filter, Identity};
use std::process::Command;

#[test]
fn device_identity_is_manufacturer_neutral_until_the_user_selects_a_vendor() {
    let filter = Filter::default();
    for vendor in [0x04e8, 0x18d1, 0x2717, 0x22b8, 0x1234] {
        assert!(filter.includes(&Identity {
            vendor,
            product: 0x1234,
            serial: None
        }));
    }
    let samsung = Filter {
        vendor: Some(0x04e8),
        ..Filter::default()
    };
    assert!(samsung.includes(&Identity {
        vendor: 0x04e8,
        product: 0x1234,
        serial: None
    }));
    assert!(!samsung.includes(&Identity {
        vendor: 0x18d1,
        product: 0x1234,
        serial: None
    }));
}

#[test]
fn combined_filters_prevent_product_collisions_and_require_an_exact_serial() {
    let filter = Filter {
        vendor: Some(0x18d1),
        product: Some(0x4e13),
        serial: Some("chosen".into()),
    };
    assert!(filter.includes(&Identity {
        vendor: 0x18d1,
        product: 0x4e13,
        serial: Some("chosen")
    }));
    for identity in [
        Identity {
            vendor: 0x04e8,
            product: 0x4e13,
            serial: Some("chosen"),
        },
        Identity {
            vendor: 0x18d1,
            product: 0x4e14,
            serial: Some("chosen"),
        },
        Identity {
            vendor: 0x18d1,
            product: 0x4e13,
            serial: None,
        },
        Identity {
            vendor: 0x18d1,
            product: 0x4e13,
            serial: Some("chosen-extra"),
        },
    ] {
        assert!(!filter.includes(&identity));
    }
}

#[test]
fn cli_rejects_malformed_and_duplicate_device_filters_before_usb_access() {
    for args in [
        vec!["--help", "--vendor", "123"],
        vec!["--help", "--vendor", "+123"],
        vec!["--help", "--vendor", "fffff"],
        vec!["--help", "--vendor", "zzzz"],
        vec!["--help", "--product", "+123"],
        vec!["--help", "--vendor", "18d1", "--vendor", "04e8"],
        vec!["--help", "--product", "1234", "--product", "5678"],
        vec!["--help", "--serial", "first", "--serial", "second"],
        vec!["--help", "--serial", "bad\nvalue"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_galaxybridge"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
    }
    let help = Command::new(env!("CARGO_BIN_EXE_galaxybridge"))
        .args([
            "--help",
            "--vendor",
            "18D1",
            "--product",
            "1234",
            "--serial",
            "chosen",
        ])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("regardless of manufacturer"));
    assert!(text.contains("do not authenticate"));
}
