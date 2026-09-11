//! Bounded datagrams across the root/USB-worker boundary.
use crate::{Result, rndis::packet::FRAME_LIMIT};
use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::UnixDatagram,
    },
};

pub struct Channel;
impl Channel {
    pub fn inherit() -> Result<UnixDatagram> {
        Self::duplicate(3)
    }

    fn duplicate(source: RawFd) -> Result<UnixDatagram> {
        // Duplication validates the numeric descriptor before Rust takes any
        // ownership. In particular, a manually invoked worker with no FD 3
        // must return an error rather than construct an invalid OwnedFd.
        let duplicate = unsafe { libc::fcntl(source, libc::F_DUPFD_CLOEXEC, 4) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: fcntl just returned a fresh descriptor owned by this call.
        let descriptor = unsafe { OwnedFd::from_raw_fd(duplicate) };
        let mut kind: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
        // SAFETY: both pointers refer to live, correctly sized values.
        if unsafe {
            libc::getsockopt(
                descriptor.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                (&mut kind as *mut libc::c_int).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if kind != libc::SOCK_DGRAM || length as usize != std::mem::size_of_val(&kind) {
            return Err("worker channel must be a datagram socket".into());
        }
        let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
        // A connected Unix peer is required; an ordinary file, stream socket,
        // UDP socket or unconnected datagram is not the supervisor channel.
        if unsafe {
            libc::getpeername(
                descriptor.as_raw_fd(),
                (&mut address as *mut libc::sockaddr_storage).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
        if address.ss_family as libc::c_int != libc::AF_UNIX {
            return Err("worker channel must have a connected Unix peer".into());
        }
        Ok(UnixDatagram::from(descriptor))
    }

    pub fn create() -> Result<(UnixDatagram, UnixDatagram)> {
        let pair = UnixDatagram::pair()?;
        for socket in [&pair.0, &pair.1] {
            let size: libc::c_int = 262144;
            for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
                // SAFETY: a valid socket, pointer to an initialized int, and its exact size.
                if unsafe {
                    libc::setsockopt(
                        socket.as_raw_fd(),
                        libc::SOL_SOCKET,
                        option,
                        (&size as *const libc::c_int).cast(),
                        std::mem::size_of_val(&size) as libc::socklen_t,
                    )
                } != 0
                {
                    return Err(io::Error::last_os_error().into());
                }
            }
        }
        Ok(pair)
    }
}

pub struct Backpressure;
impl Backpressure {
    pub fn contains(error: &io::Error) -> bool {
        matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        ) || error.raw_os_error() == Some(libc::ENOBUFS)
    }
}

pub enum Message<'a> {
    Ready([u8; 6]),
    Frame(&'a [u8]),
}
impl<'a> Message<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        match bytes.first() {
            Some(1) if bytes.len() == 7 => {
                let mac: [u8; 6] = bytes[1..].try_into()?;
                if mac[0] & 1 != 0 || mac == [0; 6] {
                    return Err("invalid unicast MAC address".into());
                }
                Ok(Self::Ready(mac))
            }
            Some(2) if (15..=FRAME_LIMIT + 1).contains(&bytes.len()) => {
                Ok(Self::Frame(&bytes[1..]))
            }
            _ => Err("invalid worker datagram".into()),
        }
    }
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Ready(mac) => [vec![1], mac.to_vec()].concat(),
            Self::Frame(frame) => [vec![2], frame.to_vec()].concat(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;
    use std::{
        fs::File,
        net::UdpSocket,
        os::{
            fd::AsRawFd,
            unix::net::{UnixDatagram, UnixStream},
        },
    };

    #[test]
    fn worker_channel_rejects_invalid_or_unrelated_descriptors_without_taking_them() {
        assert!(Channel::duplicate(-1).is_err());
        let file = File::open("/dev/null").unwrap();
        assert!(Channel::duplicate(file.as_raw_fd()).is_err());
        assert!(file.metadata().is_ok());
        let (stream, _) = UnixStream::pair().unwrap();
        assert!(Channel::duplicate(stream.as_raw_fd()).is_err());
        let unconnected = UnixDatagram::unbound().unwrap();
        assert!(Channel::duplicate(unconnected.as_raw_fd()).is_err());
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.connect(udp.local_addr().unwrap()).unwrap();
        assert!(Channel::duplicate(udp.as_raw_fd()).is_err());
    }

    #[test]
    fn worker_channel_copies_a_connected_unix_datagram_and_preserves_the_original() {
        let (source, peer) = UnixDatagram::pair().unwrap();
        let copy = Channel::duplicate(source.as_raw_fd()).unwrap();
        copy.send(b"copy").unwrap();
        let mut bytes = [0u8; 16];
        let length = peer.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..length], b"copy");
        drop(copy);
        source.send(b"original").unwrap();
        let length = peer.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..length], b"original");
    }
}
