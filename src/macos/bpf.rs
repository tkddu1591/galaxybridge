use crate::{Result, rndis::packet::FRAME_LIMIT};
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, RawFd},
};

unsafe extern "C" {
    fn gb_bpf_open(name: *const libc::c_char, size: *mut u32) -> libc::c_int;
    fn gb_bpf_layout(result: *mut u32);
}

pub struct Device {
    file: File,
    pub buffer_size: usize,
    layout: [u32; 4],
}
impl Device {
    pub fn open(interface: &str) -> Result<Self> {
        let name = CString::new(interface)?;
        let mut size = 0;
        // SAFETY: pointers reference live, correctly sized local values. C returns an owned FD.
        let fd = unsafe { gb_bpf_open(name.as_ptr(), &mut size) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !(4096..=524288).contains(&size) {
            return Err("unexpected BPF buffer size".into());
        }
        let mut layout = [0; 4];
        unsafe { gb_bpf_layout(layout.as_mut_ptr()) };
        Ok(Self {
            file,
            buffer_size: size as usize,
            layout,
        })
    }
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
    pub fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
    pub fn write(&mut self, frame: &[u8]) -> std::io::Result<()> {
        if !Frame::permits(frame) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported Ethernet frame",
            ));
        }
        // A BPF write is a packet operation. Retrying a short write with the
        // remaining bytes would inject them as an unrelated Ethernet packet.
        if self.file.write(frame)? != frame.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "short BPF packet write",
            ));
        }
        Ok(())
    }
    pub fn frames<'a>(&self, bytes: &'a [u8]) -> Result<Vec<&'a [u8]>> {
        Batch::decode(bytes, self.layout)
    }
}

/// The bridge deliberately supports IPv4 and ARP only. In particular, it must
/// not inject unsolicited IPv6 router advertisements or nested VLAN frames.
pub struct Frame;
impl Frame {
    pub fn permits(bytes: &[u8]) -> bool {
        (14..=FRAME_LIMIT).contains(&bytes.len())
            && matches!(bytes.get(12..14), Some([0x08, 0x00] | [0x08, 0x06]))
    }
}

pub struct Batch;
impl Batch {
    pub fn decode(mut bytes: &[u8], layout: [u32; 4]) -> Result<Vec<&[u8]>> {
        let [capture, original, header, alignment] = layout.map(|v| v as usize);
        if alignment != 4 || header > 64 || capture > 64 || original > 64 {
            return Err("unsupported BPF header layout".into());
        }
        let mut frames = Vec::new();
        while !bytes.is_empty() {
            let caplen = u32::from_ne_bytes(
                bytes
                    .get(capture..capture + 4)
                    .ok_or("truncated BPF capture length")?
                    .try_into()?,
            ) as usize;
            let datalen = u32::from_ne_bytes(
                bytes
                    .get(original..original + 4)
                    .ok_or("truncated BPF data length")?
                    .try_into()?,
            ) as usize;
            let hdrlen = u16::from_ne_bytes(
                bytes
                    .get(header..header + 2)
                    .ok_or("truncated BPF header length")?
                    .try_into()?,
            ) as usize;
            if hdrlen < header + 2 || caplen != datalen || !(14..=FRAME_LIMIT).contains(&caplen) {
                return Err("invalid or truncated BPF frame".into());
            }
            let end = hdrlen.checked_add(caplen).ok_or("BPF length overflow")?;
            frames.push(
                bytes
                    .get(hdrlen..end)
                    .ok_or("BPF frame outside read buffer")?,
            );
            if end == bytes.len() {
                break;
            }
            let next = end
                .checked_add(alignment - 1)
                .ok_or("BPF alignment overflow")?
                & !(alignment - 1);
            bytes = bytes.get(next..).ok_or("invalid BPF padding")?;
        }
        Ok(frames)
    }
}
