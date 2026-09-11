use galaxybridge::ipc::{Backpressure, Channel};
use std::io;

#[test]
fn macos_buffer_exhaustion_is_transient_but_disconnect_is_not() {
    assert!(Backpressure::contains(&io::Error::from_raw_os_error(
        libc::ENOBUFS
    )));
    assert!(Backpressure::contains(&io::Error::from(
        io::ErrorKind::WouldBlock
    )));
    assert!(!Backpressure::contains(&io::Error::from_raw_os_error(
        libc::ECONNRESET
    )));
    assert!(!Backpressure::contains(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
}

#[test]
fn saturated_datagram_channel_has_a_bounded_queue_and_can_resume() {
    let (sender, receiver) = Channel::create().unwrap();
    sender.set_nonblocking(true).unwrap();
    receiver.set_nonblocking(true).unwrap();
    let frame = [2u8; 1515];
    let mut full = false;
    for _ in 0..4096 {
        if let Err(error) = sender.send(&frame) {
            assert!(Backpressure::contains(&error));
            full = true;
            break;
        }
    }
    assert!(
        full,
        "queue must exert backpressure rather than grow without bound"
    );
    let mut bytes = [0; 1600];
    while receiver.recv(&mut bytes).is_ok() {}
    assert_eq!(sender.send(&frame).unwrap(), frame.len());
}
