use super::{control::TRANSFER_LIMIT, wire};
use crate::Result;

pub const FRAME_LIMIT: usize = 1514;
pub const HEADER: usize = 44;

pub fn encode(frame: &[u8], usb_packet_size: usize, max_transfer: usize) -> Result<Vec<u8>> {
    if !(14..=FRAME_LIMIT).contains(&frame.len()) || usb_packet_size == 0 {
        return Err("invalid Ethernet frame".into());
    }
    let len = HEADER + frame.len();
    // A short USB packet terminates the transfer without requiring a separate ZLP.
    let padded_len = len + usize::from(len % usb_packet_size == 0);
    if padded_len > max_transfer || padded_len > TRANSFER_LIMIT {
        return Err("outbound transfer too large".into());
    }
    let mut msg = wire::words(&[1, len as u32, 36, frame.len() as u32, 0, 0, 0, 0, 0, 0, 0]);
    msg.extend_from_slice(frame);
    msg.resize(padded_len, 0);
    Ok(msg)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    if bytes.len() > TRANSFER_LIMIT {
        return Err("USB transfer exceeds negotiated bound".into());
    }
    let mut rest = bytes;
    let mut frames = Vec::new();
    while !rest.is_empty() {
        // USB transports may append a small, all-zero terminator/padding.
        if rest.iter().all(|b| *b == 0) {
            break;
        }
        if rest.len() < HEADER || wire::u32_at(rest, 0)? != 1 {
            return Err("invalid packet message".into());
        }
        let len = wire::u32_at(rest, 4)? as usize;
        if len < HEADER || len > rest.len() {
            return Err("invalid packet message length".into());
        }
        let msg = &rest[..len];
        let start = (wire::u32_at(msg, 8)? as usize)
            .checked_add(8)
            .ok_or("data offset overflow")?;
        let data_len = wire::u32_at(msg, 12)? as usize;
        let end = start.checked_add(data_len).ok_or("data length overflow")?;
        if start < HEADER || !(14..=FRAME_LIMIT).contains(&data_len) {
            return Err("invalid Ethernet payload range".into());
        }
        // Connectionless Ethernet only; reject unsupported auxiliary metadata.
        for offset in [16, 20, 24, 28, 32, 36, 40] {
            if wire::u32_at(msg, offset)? != 0 {
                return Err("unsupported RNDIS packet metadata".into());
            }
        }
        frames.push(
            msg.get(start..end)
                .ok_or("payload outside packet message")?,
        );
        rest = &rest[len..];
    }
    Ok(frames)
}
