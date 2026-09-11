//! Command-line selection shared by the supervisor and its separate USB program.
use crate::{Result, usb::devices::Filter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Help,
    Version,
    Devices,
    Handshake,
    Connect,
    Daemon,
    Probe,
    Worker,
}
impl Mode {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "help" | "--help" | "-h" => Self::Help,
            "version" | "--version" => Self::Version,
            "devices" => Self::Devices,
            "handshake" => Self::Handshake,
            "connect" => Self::Connect,
            "daemon" => Self::Daemon,
            "probe" => Self::Probe,
            "worker" => Self::Worker,
            _ => return Err("unknown command; use galaxybridge --help".into()),
        })
    }
}

pub struct Options {
    pub mode: Mode,
    pub filter: Filter,
    pub show_serial: bool,
}
impl Options {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mode = Mode::parse(args.next().as_deref().unwrap_or("help"))?;
        let mut filter = Filter::default();
        let mut show_serial = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--vendor" | "--product" => {
                    let value = args.next().ok_or("USB ID needs four hexadecimal digits")?;
                    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err("USB ID must have four hexadecimal digits".into());
                    }
                    let id = Some(u16::from_str_radix(&value, 16)?);
                    let selected = if arg == "--vendor" {
                        &mut filter.vendor
                    } else {
                        &mut filter.product
                    };
                    if selected.is_some() {
                        return Err("duplicate USB ID filter".into());
                    }
                    *selected = id;
                }
                "--serial" => {
                    let serial = args.next().ok_or("--serial needs a value")?;
                    if serial.is_empty()
                        || serial.len() > 128
                        || !serial.bytes().all(|b| b.is_ascii_graphic())
                    {
                        return Err("serial must be 1-128 printable ASCII characters".into());
                    }
                    if filter.serial.replace(serial).is_some() {
                        return Err("duplicate serial filter".into());
                    }
                }
                "--show-serial" if mode == Mode::Devices => show_serial = true,
                _ => return Err(format!("unknown argument: {arg:?}").into()),
            }
        }

        Ok(Self {
            mode,
            filter,
            show_serial,
        })
    }
}
