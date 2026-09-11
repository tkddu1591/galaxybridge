use crate::{
    Result,
    ipc::{Backpressure, Message},
    rndis::{control::TRANSFER_LIMIT, packet},
    usb::{devices::Filter, session::Session},
};
use nusb::transfer::Buffer;
use std::{
    io::ErrorKind,
    os::unix::net::UnixDatagram,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub fn run(socket: UnixDatagram, filter: Filter, stop: Arc<AtomicBool>) -> Result<()> {
    // USB descriptor and protocol parsing happen only in this unprivileged process.
    if unsafe { libc::geteuid() } == 0 {
        return Err("USB worker must not run as root".into());
    }
    let supervisor = unsafe { libc::getppid() };
    if supervisor <= 1 {
        return Err("USB worker has no live supervisor".into());
    }
    let mut session = Session::open(&filter)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    socket.set_write_timeout(Some(Duration::from_millis(100)))?;
    socket.send(&Message::Ready(session.mac).encode())?;
    let rx_socket = socket.try_clone()?;
    let mut incoming = session.incoming.take().ok_or("missing incoming endpoint")?;
    let mut outgoing = session.outgoing.take().ok_or("missing outgoing endpoint")?;
    let max_transfer = session.max_transfer;
    for _ in 0..8 {
        incoming.submit(Buffer::new(TRANSFER_LIMIT));
    }

    std::thread::scope(|scope| -> Result<()> {
        let rx_stop = stop.clone();
        let receive = scope.spawn(move || -> Result<()> {
            let result = (|| -> Result<()> {
                while !rx_stop.load(Ordering::Relaxed) {
                    let Some(completion) = incoming.wait_next_complete(Duration::from_millis(100))
                    else {
                        continue;
                    };
                    let mut buffer = completion.into_result()?;
                    let frames = packet::decode(&buffer)?;
                    for frame in frames {
                        if rx_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        match rx_socket.send(&Message::Frame(frame).encode()) {
                            Ok(_) => (),
                            Err(e) if Backpressure::contains(&e) => {}
                            Err(e) => return Err(e.into()),
                        }
                    }
                    buffer.clear();
                    incoming.submit(buffer);
                }
                Ok(())
            })();
            incoming.cancel_all();
            rx_stop.store(true, Ordering::Relaxed);
            result
        });
        let tx_stop = stop.clone();
        let transmit = scope.spawn(move || -> Result<()> {
            let result = (|| -> Result<()> {
                let mut bytes = [0u8; 1600];
                while !tx_stop.load(Ordering::Relaxed) {
                    let n = match socket.recv(&mut bytes) {
                        Ok(n) => n,
                        Err(e)
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                        {
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    };
                    let Message::Frame(frame) = Message::decode(&bytes[..n])? else {
                        return Err("unexpected root datagram".into());
                    };
                    let encoded = packet::encode(frame, outgoing.max_packet_size(), max_transfer)?;
                    let mut buffer = Buffer::new(encoded.len());
                    buffer.extend_from_slice(&encoded);
                    // One bounded transfer at a time avoids unbounded TX queues.
                    outgoing
                        .transfer_blocking(buffer, Duration::from_millis(500))
                        .into_result()?;
                }
                Ok(())
            })();
            tx_stop.store(true, Ordering::Relaxed);
            result
        });
        let mut last_check = Instant::now();
        let mut failure = None;
        while !stop.load(Ordering::Relaxed) {
            // Do not retain USB ownership if the supervisor is force-killed.
            if unsafe { libc::getppid() } != supervisor {
                break;
            }
            if receive.is_finished() || transmit.is_finished() {
                break;
            }
            if last_check.elapsed() >= Duration::from_secs(3) {
                if let Err(error) = session.keepalive() {
                    failure = Some(error);
                    break;
                }
                last_check = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        stop.store(true, Ordering::Relaxed);
        let rx = receive.join().map_err(|_| "USB RX thread panicked")?;
        let tx = transmit.join().map_err(|_| "USB TX thread panicked")?;
        if let Some(error) = failure {
            return Err(error);
        }
        rx?;
        tx?;
        Ok(())
    })
}
