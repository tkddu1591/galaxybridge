use galaxybridge::{Result, usb::devices::Filter};
use std::{
    os::{fd::FromRawFd, unix::net::UnixDatagram},
    sync::{Arc, atomic::AtomicBool},
};

fn main() {
    if let Err(error) = entry() {
        eprintln!("GalaxyBridge: {error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "help".into());
    let mut filter = Filter::default();
    let mut show_serial = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--product" => {
                let value = args
                    .next()
                    .ok_or("--product needs four hexadecimal digits")?;
                if value.len() != 4 {
                    return Err("product ID must have four hexadecimal digits".into());
                }
                filter.product = Some(u16::from_str_radix(&value, 16)?);
            }
            "--serial" => {
                let serial = args.next().ok_or("--serial needs a value")?;
                if serial.is_empty()
                    || serial.len() > 128
                    || !serial.bytes().all(|b| b.is_ascii_graphic())
                {
                    return Err("serial must be 1-128 printable ASCII characters".into());
                }
                filter.serial = Some(serial);
            }
            "--show-serial" if mode == "devices" => show_serial = true,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    match mode.as_str() {
        "--version" | "version" => println!(
            "GalaxyBridge {} · independent RNDIS driver",
            env!("CARGO_PKG_VERSION")
        ),
        "devices" => {
            let devices = filter.list()?;
            for device in &devices {
                print!("Samsung RNDIS 04e8:{:04x}", device.product_id());
                if show_serial {
                    print!(
                        " serial={:?}",
                        device.serial_number().unwrap_or("unavailable")
                    );
                }
                println!();
            }
            println!("{} matching device(s)", devices.len());
        }
        "probe" => {
            if filter.list()?.len() != 1 {
                std::process::exit(2);
            }
        }
        "handshake" => {
            if unsafe { libc::geteuid() } == 0 {
                return Err("run handshake without sudo".into());
            }
            let _session = galaxybridge::usb::session::Session::open(&filter)?;
            println!("RNDIS initialization completed without root privileges");
        }
        "connect" | "daemon" | "worker" => {
            let stop = Arc::new(AtomicBool::new(false));
            for signal in [
                signal_hook::consts::SIGINT,
                signal_hook::consts::SIGTERM,
                signal_hook::consts::SIGHUP,
            ] {
                signal_hook::flag::register(signal, stop.clone())?;
            }
            if mode == "worker" {
                // SAFETY: this internal command is spawned with an owned socket at FD 3.
                // A manually invoked worker cannot gain privileges and invalid FDs fail on use.
                let socket = unsafe { UnixDatagram::from_raw_fd(3) };
                galaxybridge::worker::run(socket, filter, stop)?;
            } else {
                #[cfg(target_os = "macos")]
                galaxybridge::service::Supervisor::new(filter, stop)?.run(mode == "daemon")?;
                #[cfg(not(target_os = "macos"))]
                return Err("network interfaces are supported only on macOS".into());
            }
        }
        "help" | "--help" | "-h" => println!(
            "GalaxyBridge — independent Galaxy USB tethering for Apple Silicon\n\n  galaxybridge devices [--show-serial]\n  sudo galaxybridge connect [--product HEX] [--serial SERIAL]\n  sudo galaxybridge daemon [--product HEX] [--serial SERIAL]\n\nUse the release installer for automatic reconnect. SIP and USB debugging stay enabled/disabled as they were; no security policy changes are required."
        ),
        _ => return Err("unknown command; use galaxybridge --help".into()),
    }
    Ok(())
}
