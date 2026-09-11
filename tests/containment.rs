use std::process::Command;

#[test]
fn supervisor_refuses_internal_usb_modes_instead_of_parsing_device_data() {
    for mode in ["worker", "probe"] {
        let output = Command::new(env!("CARGO_BIN_EXE_galaxybridge"))
            .arg(mode)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("internal USB modes"));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn unsigned_development_helper_fails_closed_before_usb_enumeration() {
    let output = Command::new(env!("CARGO_BIN_EXE_galaxybridge-usb"))
        .arg("devices")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("App Sandbox worker") || error.contains("without root privileges"));
}
