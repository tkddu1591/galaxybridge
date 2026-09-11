use crate::{
    Result,
    ipc::{Backpressure, Channel, Message},
    macos::{
        bpf::Device,
        interface::Pair,
        process::Worker,
        route::{Recovery, Snapshot},
    },
    usb::devices::Filter,
};
use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub struct Supervisor {
    filter: Filter,
    stop: Arc<AtomicBool>,
    _lock: File,
}
struct Connection {
    worker: Option<Worker>,
    bpf: Option<Device>,
    pair: Pair,
    previous: Option<Snapshot>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        let started = Instant::now();
        let owned = self.pair.system.clone();
        eprintln!(
            "GalaxyBridge: disconnecting {owned}; fallback interface: {}",
            self.previous
                .as_ref()
                .map_or("none", |route| route.interface.as_str())
        );
        // No more USB frames may arrive while IPConfiguration removes the
        // temporary service. Restore fallback routes only after that removal.
        drop(self.worker.take());
        drop(self.bpf.take());
        eprintln!(
            "GalaxyBridge: transport stopped after {:?}",
            started.elapsed()
        );
        if let Err(error) = self.pair.remove() {
            eprintln!("GalaxyBridge: interface cleanup: {error}");
        }
        eprintln!(
            "GalaxyBridge: interface cleanup completed after {:?}",
            started.elapsed()
        );
        if let Err(error) = Recovery::restore(self.previous.as_ref(), &owned) {
            eprintln!("GalaxyBridge: fallback recovery: {error}");
        }
        eprintln!(
            "GalaxyBridge: fallback check completed after {:?}",
            started.elapsed()
        );
    }
}

impl Supervisor {
    pub fn new(filter: Filter, stop: Arc<AtomicBool>) -> Result<Self> {
        if unsafe { libc::geteuid() } != 0 {
            return Err("network setup requires root; use sudo galaxybridge connect".into());
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/var/run/io.galaxybridge.lock")?;
        if lock.metadata()?.uid() != 0 || lock.metadata()?.nlink() != 1 {
            return Err("unsafe lock file".into());
        }
        // SAFETY: flock acts only on this owned descriptor; no pointer arguments.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("another GalaxyBridge supervisor is running".into());
        }
        Ok(Self {
            filter,
            stop,
            _lock: lock,
        })
    }

    pub fn run(&self, repeat: bool) -> Result<()> {
        eprintln!("GalaxyBridge: waiting for one matching Samsung RNDIS device");
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            match Worker::probe(&self.filter) {
                Ok(true) => {
                    let result = self.connect();
                    if !repeat {
                        return result;
                    }
                    if let Err(error) = result {
                        eprintln!("GalaxyBridge: session ended: {error}");
                    }
                }
                Ok(false) if !repeat => {
                    return Err(
                        "no unique device available; check USB tethering or device selection"
                            .into(),
                    );
                }
                Err(error) if !repeat => return Err(error),
                Err(error) => eprintln!("GalaxyBridge: discovery failed: {error}"),
                _ => (),
            }
            for _ in 0..30 {
                if self.stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    fn connect(&self) -> Result<()> {
        let previous = Snapshot::read()?;
        let pair = Pair::create()?;
        let bpf = Device::open(&pair.transport)?;
        let (socket, child_socket) = Channel::create()?;
        socket.set_nonblocking(true)?;
        let worker = Worker::spawn(&self.filter, &child_socket)?;
        drop(child_socket);
        let mut bpf_buffer = vec![0u8; bpf.buffer_size];
        let mut connection = Connection {
            worker: Some(worker),
            bpf: Some(bpf),
            pair,
            previous,
        };
        let mut ipc_buffer = [0u8; 1600];
        let started = Instant::now();
        let mut ready = false;
        let mut network_check = Instant::now();
        let mut has_address = false;
        let mut gateway_failures = 0u8;
        let mut sent_frames = 0u64;
        let mut received_frames = 0u64;

        while !self.stop.load(Ordering::Relaxed) {
            let worker = connection.worker.as_mut().ok_or("USB worker is closed")?;
            let bpf = connection
                .bpf
                .as_mut()
                .ok_or("network transport is closed")?;
            if let Some(status) = worker.child.try_wait()? {
                return Err(format!("USB worker exited ({status})").into());
            }
            if !ready && started.elapsed() > Duration::from_secs(10) {
                return Err("USB initialization timed out".into());
            }
            let mut fds = [
                libc::pollfd {
                    fd: socket.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: bpf.fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: fds is a live two-element pollfd array, length matches the pointer.
            if unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) } < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            if fds
                .iter()
                .any(|f| f.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0)
            {
                return Err("network transport closed".into());
            }
            if fds[0].revents & libc::POLLIN != 0 {
                for _ in 0..128 {
                    let n = match socket.recv(&mut ipc_buffer) {
                        Ok(n) => n,
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e.into()),
                    };
                    match Message::decode(&ipc_buffer[..n])? {
                        Message::Ready(mac) if !ready => {
                            connection.pair.activate(mac)?;
                            connection.pair.dhcp()?;
                            ready = true;
                            eprintln!(
                                "GalaxyBridge: USB ready on {} (worker has no root privileges)",
                                connection.pair.system
                            );
                        }
                        Message::Frame(frame) if ready => match bpf.write(frame) {
                            Ok(()) => received_frames += 1,
                            Err(e) if Backpressure::contains(&e) => {}
                            Err(e) => return Err(e.into()),
                        },
                        _ => return Err("unexpected worker protocol state".into()),
                    }
                }
            }
            if fds[1].revents & libc::POLLIN != 0 {
                match bpf.read(&mut bpf_buffer) {
                    Ok(n) if ready => {
                        for frame in bpf.frames(&bpf_buffer[..n])? {
                            sent_frames += 1;
                            match socket.send(&Message::Frame(frame).encode()) {
                                Ok(_) => (),
                                Err(e) if Backpressure::contains(&e) => (),
                                Err(e) => return Err(e.into()),
                            }
                        }
                    }
                    Ok(_) => (),
                    Err(e)
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            if ready && network_check.elapsed() >= Duration::from_secs(3) {
                if let Some(phone) = Snapshot::dhcp(&connection.pair.system) {
                    let current = Snapshot::read()?;
                    let vpn =
                        current.as_ref().is_some_and(Snapshot::is_vpn) || Snapshot::vpn_present()?;
                    if !vpn && current.as_ref() != Some(&phone) {
                        if current
                            .as_ref()
                            .is_some_and(|s| s.interface != connection.pair.system)
                        {
                            connection.previous = current;
                        }
                        phone.apply()?;
                    }
                    if !has_address {
                        if vpn {
                            eprintln!("GalaxyBridge: DHCP ready; VPN routes preserved");
                        } else {
                            eprintln!("GalaxyBridge: DHCP ready; phone is the preferred IPv4 path");
                        }
                    }
                    has_address = true;
                    gateway_failures = 0;
                } else {
                    has_address = false;
                    gateway_failures += 1;
                    eprintln!(
                        "GalaxyBridge: waiting for DHCP (frames to USB: {sent_frames}, from USB: {received_frames})"
                    );
                    if gateway_failures >= 10 {
                        return Err("DHCP did not complete".into());
                    }
                    if gateway_failures % 3 == 0 {
                        connection.pair.dhcp()?;
                    }
                }
                network_check = Instant::now();
            }
        }
        Ok(())
    }
}
