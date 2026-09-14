//! Enter the worker account's Mach bootstrap before creating supervisor resources.
use super::{
    access::{Core, TrustedPath},
    command,
    identity::Account,
};
use crate::{Result, cli::Mode, usb::devices::Filter};
use std::{os::unix::process::CommandExt, path::Path, process::Command};

const EXECUTABLE: &str = "/Library/PrivilegedHelperTools/io.galaxybridge/galaxybridge";
const EXPECTED_PID: &str = "GALAXYBRIDGE_BOOTSTRAP_PID";

pub struct Context;
impl Context {
    /// Call before installing signal handlers or opening the supervisor lock.
    /// App Sandbox initialization requires the same bootstrap UID as the worker's
    /// effective UID. `asuser` changes that context while retaining root credentials.
    pub fn enter(mode: Mode, filter: &Filter) -> Result<()> {
        let arguments = Arguments::create(mode, filter)?;
        if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
            return Err("network setup requires root; use sudo galaxybridge connect".into());
        }
        Core::disable()?;
        let account = Account::load()?;
        let manager = command::text("/bin/launchctl", &["manageruid"])?;
        let manager = manager.parse::<libc::uid_t>()?;
        let expected = std::env::var_os(EXPECTED_PID);
        if let Some(expected) = &expected {
            Transition::check(
                account.uid,
                manager,
                expected.to_str().ok_or("invalid bootstrap PID marker")?,
                std::process::id(),
            )?;
        }
        if manager == account.uid {
            return Ok(());
        }

        // Only reopen the installed, root-protected program. Never execute a
        // caller-supplied path or a development checkout with root credentials.
        let executable = Path::new(EXECUTABLE);
        for ancestor in executable.ancestors().skip(1) {
            TrustedPath::directory(ancestor)?;
        }
        TrustedPath::file(executable, 0o755)?;
        let error = Command::new("/bin/launchctl")
            .args(["asuser", &account.uid.to_string(), EXECUTABLE])
            .args(arguments)
            .env_clear()
            .env("LC_ALL", "C")
            .env("HOME", "/var/root")
            .env(EXPECTED_PID, std::process::id().to_string())
            .current_dir("/")
            .exec();
        Err(error.into())
    }
}

struct Transition;
impl Transition {
    fn check(uid: libc::uid_t, manager: libc::uid_t, expected: &str, pid: u32) -> Result<()> {
        // Some historical launchctl versions forked and waited. Fail before
        // USB/network setup if that would break launchd's tracked PID or signals.
        if expected != pid.to_string() {
            return Err("launchctl asuser did not preserve the supervisor PID".into());
        }
        if manager != uid {
            return Err("launchctl asuser did not enter the worker bootstrap context".into());
        }
        Ok(())
    }
}

struct Arguments;
impl Arguments {
    fn create(mode: Mode, filter: &Filter) -> Result<Vec<String>> {
        let mode = match mode {
            Mode::Connect => "connect",
            Mode::Daemon => "daemon",
            _ => return Err("bootstrap entry is only for connect or daemon".into()),
        };
        let mut args = vec![mode.to_string()];
        for (flag, value) in [("--vendor", filter.vendor), ("--product", filter.product)] {
            if let Some(value) = value {
                args.extend([flag.to_string(), format!("{value:04x}")]);
            }
        }
        if let Some(serial) = &filter.serial {
            args.extend(["--serial".to_string(), serial.clone()]);
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Options;

    #[test]
    fn bootstrap_reentry_rejects_changed_pid_context_and_invalid_markers() {
        assert!(Transition::check(60000, 60000, "123", 123).is_ok());
        assert!(Transition::check(60000, 60000, "123", 124).is_err());
        assert!(Transition::check(60000, 0, "123", 123).is_err());
        for marker in ["", "0123", "123\n", "-1", "4294967296"] {
            assert!(Transition::check(60000, 60000, marker, 123).is_err());
        }
    }

    #[test]
    fn bootstrap_arguments_preserve_selection_without_shell_interpretation() {
        for mode in [Mode::Connect, Mode::Daemon] {
            let filter = Filter {
                vendor: Some(0x04e8),
                product: Some(0x6863),
                serial: Some("$(touch);'--vendor".to_string()),
            };
            let parsed =
                Options::parse(Arguments::create(mode, &filter).unwrap().into_iter()).unwrap();
            assert_eq!(parsed.mode, mode);
            assert_eq!(parsed.filter.vendor, filter.vendor);
            assert_eq!(parsed.filter.product, filter.product);
            assert_eq!(parsed.filter.serial, filter.serial);
        }
        assert!(Arguments::create(Mode::Devices, &Filter::default()).is_err());
        assert!(Arguments::create(Mode::Worker, &Filter::default()).is_err());
    }
}
