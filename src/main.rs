use galaxybridge::{
    Result,
    cli::{Mode, Options},
};
use std::sync::{Arc, atomic::AtomicBool};

fn main() {
    if let Err(error) = entry() {
        eprintln!("GalaxyBridge: {error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let Options {
        mode,
        filter,
        show_serial,
    } = Options::parse(std::env::args().skip(1))?;
    match mode {
        Mode::Version => println!(
            "GalaxyBridge {} · independent RNDIS driver",
            env!("CARGO_PKG_VERSION")
        ),
        Mode::Devices | Mode::Handshake => {
            #[cfg(target_os = "macos")]
            galaxybridge::macos::process::Worker::inspect(
                if mode == Mode::Devices {
                    "devices"
                } else {
                    "handshake"
                },
                &filter,
                show_serial,
            )?;
            #[cfg(not(target_os = "macos"))]
            return Err("USB inspection requires the installed macOS sandboxed worker".into());
        }
        Mode::Connect | Mode::Daemon => {
            #[cfg(target_os = "macos")]
            galaxybridge::macos::context::Context::enter(mode, &filter)?;
            let stop = Arc::new(AtomicBool::new(false));
            for signal in [
                signal_hook::consts::SIGINT,
                signal_hook::consts::SIGTERM,
                signal_hook::consts::SIGHUP,
            ] {
                signal_hook::flag::register(signal, stop.clone())?;
            }
            #[cfg(target_os = "macos")]
            galaxybridge::service::Supervisor::new(filter, stop)?.run(mode == Mode::Daemon)?;
            #[cfg(not(target_os = "macos"))]
            return Err("network interfaces are supported only on macOS".into());
        }
        Mode::Probe | Mode::Worker => {
            return Err("internal USB modes run only in the installed sandboxed worker".into());
        }
        Mode::Help => println!(
            "GalaxyBridge — Android USB tethering (RNDIS) for Apple Silicon\n\n  galaxybridge devices [--vendor HEX] [--product HEX] [--serial SERIAL] [--show-serial]\n  sudo galaxybridge connect [--vendor HEX] [--product HEX] [--serial SERIAL]\n  sudo galaxybridge daemon [--vendor HEX] [--product HEX] [--serial SERIAL]\n\nConnects one matching RNDIS device, regardless of manufacturer. NCM/ECM are not handled by this driver. USB identifiers select a device; they do not authenticate it. Use the release installer for optional automatic reconnect. SIP and USB debugging settings do not need to change."
        ),
    }
    Ok(())
}
