//! Bounded datagrams across the root/USB-worker boundary.
use crate::{Result, rndis::packet::FRAME_LIMIT};
use std::{
    io,
    os::{fd::AsRawFd, unix::net::UnixDatagram},
};

pub struct Channel;
impl Channel {
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
