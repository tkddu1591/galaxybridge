//! Separate executable: installed only inside the signed USBWorker.app sandbox.
use galaxybridge::{
    Result,
    cli::{Mode, Options},
    ipc::Channel,
};
use std::sync::{Arc, atomic::AtomicBool};

fn main() {
    if let Err(error) = entry() {
        eprintln!("GalaxyBridge USB: {error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let Options {
        mode,
        filter,
        show_serial,
    } = Options::parse(std::env::args().skip(1))?;
    if matches!(mode, Mode::Help | Mode::Version) {
        println!(
            "GalaxyBridge USB {} · USB worker",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if unsafe { libc::geteuid() } == 0 {
        return Err("USB commands must run without root privileges".into());
    }
    galaxybridge::usb::confinement::Confinement::check()?;
    match mode {
        Mode::Devices => {
            let devices = filter.list()?;
            for device in &devices {
                print!(
                    "RNDIS {:04x}:{:04x}",
                    device.vendor_id(),
                    device.product_id()
                );
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
        Mode::Probe => {
            if filter.list()?.len() != 1 {
                std::process::exit(2);
            }
        }
        Mode::Handshake => {
            let _session = galaxybridge::usb::session::Session::open(&filter)?;
            println!("RNDIS initialization completed in the USB worker");
        }
        Mode::Worker => {
            let socket = Channel::inherit()?;
            let stop = Arc::new(AtomicBool::new(false));
            for signal in [
                signal_hook::consts::SIGINT,
                signal_hook::consts::SIGTERM,
                signal_hook::consts::SIGHUP,
            ] {
                signal_hook::flag::register(signal, stop.clone())?;
            }
            galaxybridge::worker::run(socket, filter, stop)?;
        }
        _ => return Err("USB helper does not perform privileged network setup".into()),
    }
    Ok(())
}
