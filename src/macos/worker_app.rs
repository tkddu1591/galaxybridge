//! Fixed, signed App Sandbox boundary for every USB parsing process.
use super::{access::TrustedPath, command};
use crate::Result;
use std::path::Path;

pub const EXECUTABLE: &str =
    "/Library/PrivilegedHelperTools/io.galaxybridge/USBWorker.app/Contents/MacOS/galaxybridge-usb";
const BUNDLE: &str = "/Library/PrivilegedHelperTools/io.galaxybridge/USBWorker.app";

pub struct Bundle;
impl Bundle {
    pub fn check() -> Result<()> {
        for ancestor in Path::new(EXECUTABLE).ancestors().skip(1) {
            TrustedPath::directory(ancestor)?;
        }
        for (relative, mode) in [
            ("Contents/MacOS/galaxybridge-usb", 0o755),
            ("Contents/Info.plist", 0o644),
            ("Contents/_CodeSignature/CodeResources", 0o644),
        ] {
            TrustedPath::file(&Path::new(BUNDLE).join(relative), mode)?;
        }
        TrustedPath::directory(&Path::new(BUNDLE).join("Contents/_CodeSignature"))?;
        for target in [BUNDLE, EXECUTABLE] {
            command::text(
                "/usr/bin/codesign",
                &[
                    "--verify",
                    "--strict",
                    "--deep",
                    "-R",
                    "=identifier \"io.galaxybridge.usb-worker\"",
                    target,
                ],
            )?;
        }
        for (key, expected) in [
            ("CFBundleIdentifier", "io.galaxybridge.usb-worker"),
            ("CFBundleExecutable", "galaxybridge-usb"),
            ("CFBundlePackageType", "APPL"),
        ] {
            let info = command::text(
                "/usr/bin/plutil",
                &[
                    "-extract",
                    key,
                    "raw",
                    "-o",
                    "-",
                    &format!("{BUNDLE}/Contents/Info.plist"),
                ],
            )?;
            if info != expected {
                return Err(format!("USB worker bundle field is incorrect: {key}").into());
            }
        }
        let entitlements = command::text(
            "/usr/bin/codesign",
            &["-d", "--entitlements", "-", "--xml", EXECUTABLE],
        )?;
        Entitlements::check(&entitlements)
    }
}

pub struct Entitlements;
impl Entitlements {
    pub fn check(text: &str) -> Result<()> {
        // This deliberately accepts only codesign's canonical two-boolean
        // XML output, not a general XML language. Unknown tags, entities,
        // comments, duplicate keys and additional entitlements fail closed.
        let mut text = text.trim();
        if let Some(rest) = text.strip_prefix("<?xml version=\"1.0\" encoding=\"UTF-8\"?>") {
            text = rest.trim_start();
        }
        for declaration in [
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"https://www.apple.com/DTDs/PropertyList-1.0.dtd\">",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">",
        ] {
            if let Some(rest) = text.strip_prefix(declaration) {
                text = rest.trim_start();
                break;
            }
        }
        text = text
            .strip_prefix("<plist version=\"1.0\">")
            .ok_or("unrecognized worker entitlement plist")?
            .trim_start();
        text = text
            .strip_prefix("<dict>")
            .ok_or("worker entitlements must be a dictionary")?
            .trim_start();
        let mut sandbox = false;
        let mut usb = false;
        for _ in 0..2 {
            text = text
                .strip_prefix("<key>")
                .ok_or("missing worker entitlement key")?;
            let (key, rest) = text
                .split_once("</key>")
                .ok_or("unterminated worker entitlement key")?;
            text = rest
                .trim_start()
                .strip_prefix("<true/>")
                .ok_or("worker entitlements must be true booleans")?
                .trim_start();
            match key {
                "com.apple.security.app-sandbox" if !sandbox => sandbox = true,
                "com.apple.security.device.usb" if !usb => usb = true,
                _ => return Err("unknown or duplicate USB worker entitlement".into()),
            }
        }
        text = text
            .strip_prefix("</dict>")
            .ok_or("additional USB worker entitlements are forbidden")?
            .trim_start();
        if !sandbox || !usb || text != "</plist>" {
            return Err("USB worker sandbox policy is incomplete or malformed".into());
        }
        Ok(())
    }
}
