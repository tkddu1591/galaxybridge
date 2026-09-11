use super::wire;
use crate::Result;

pub const TRANSFER_LIMIT: usize = 16_384;
pub const MAC_ADDRESS: u32 = 0x0101_0102;
pub const PACKET_FILTER: u32 = 0x0001_010e;

pub struct Request {
    pub id: u32,
    pub kind: u32,
    pub bytes: Vec<u8>,
}

impl Request {
    pub fn initialize(id: u32) -> Self {
        Self {
            id,
            kind: 2,
            bytes: wire::words(&[2, 24, id, 1, 0, TRANSFER_LIMIT as u32]),
        }
    }
    pub fn query(id: u32, oid: u32) -> Self {
        Self {
            id,
            kind: 4,
            bytes: wire::words(&[4, 28, id, oid, 0, 0, 0]),
        }
    }
    pub fn filter(id: u32) -> Self {
        // Directed + multicast + all multicast + broadcast. No promiscuous mode.
        Self {
            id,
            kind: 5,
            bytes: wire::words(&[5, 32, id, PACKET_FILTER, 4, 20, 0, 0x0f]),
        }
    }
    pub fn keepalive(id: u32) -> Self {
        Self {
            id,
            kind: 8,
            bytes: wire::words(&[8, 12, id]),
        }
    }
    pub fn halt(id: u32) -> Self {
        Self {
            id,
            kind: 3,
            bytes: wire::words(&[3, 12, id]),
        }
    }
    pub fn response<'a>(&self, bytes: &'a [u8]) -> Result<&'a [u8]> {
        let minimum = match self.kind {
            2 => 52,
            4 => 24,
            5 | 8 => 16,
            _ => return Err("unsupported RNDIS completion type".into()),
        };
        let msg = Message::parse(bytes, self.kind | 0x8000_0000, minimum)?;
        if wire::u32_at(msg, 8)? != self.id {
            return Err("mismatched control completion".into());
        }
        if wire::u32_at(msg, 12)? != 0 {
            return Err("device rejected RNDIS request".into());
        }
        Ok(msg)
    }
}

pub struct Negotiation {
    pub max_transfer: usize,
}
impl Negotiation {
    pub fn parse(msg: &[u8]) -> Result<Self> {
        let msg = Message::parse(msg, 0x8000_0002, 52)?;
        if wire::u32_at(msg, 12)? != 0 {
            return Err("RNDIS initialization was rejected".into());
        }
        if wire::u32_at(msg, 16)? != 1
            || wire::u32_at(msg, 20)? != 0
            || wire::u32_at(msg, 24)? != 1
            || wire::u32_at(msg, 28)? != 0
            || wire::u32_at(msg, 44)? != 0
            || wire::u32_at(msg, 48)? != 0
        {
            return Err("unsupported RNDIS version or medium".into());
        }
        let max_transfer = (wire::u32_at(msg, 36)? as usize).min(TRANSFER_LIMIT);
        if max_transfer < 1558 || wire::u32_at(msg, 32)? == 0 || wire::u32_at(msg, 40)? > 7 {
            return Err("invalid device transfer limits".into());
        }
        Ok(Self { max_transfer })
    }
}

pub struct Query;
impl Query {
    pub fn data(msg: &[u8]) -> Result<&[u8]> {
        let msg = Message::parse(msg, 0x8000_0004, 24)?;
        if wire::u32_at(msg, 12)? != 0 {
            return Err("RNDIS query was rejected".into());
        }
        let len = wire::u32_at(msg, 16)? as usize;
        if len == 0 {
            if wire::u32_at(msg, 20)? != 0 {
                return Err("empty query result has a nonzero offset".into());
            }
            return Ok(&[]);
        }
        let start = (wire::u32_at(msg, 20)? as usize)
            .checked_add(8)
            .ok_or("offset overflow")?;
        let end = start.checked_add(len).ok_or("length overflow")?;
        if start < 24 {
            return Err("query data overlaps header".into());
        }
        msg.get(start..end)
            .ok_or_else(|| "query data outside message".into())
    }
}

pub struct Notification;
impl Notification {
    pub fn validate(bytes: &[u8]) -> Result<()> {
        if bytes.len() != 8 || wire::u32_at(bytes, 0)? != 1 || wire::u32_at(bytes, 4)? != 0 {
            return Err("invalid RESPONSE_AVAILABLE notification".into());
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Indication {
    MediaConnect,
    MediaDisconnect,
    Other,
}

impl Indication {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let msg = Message::parse(bytes, 7, 20)?;
        let length = wire::u32_at(msg, 12)? as usize;
        let start = wire::u32_at(msg, 16)? as usize;
        if length == 0 {
            if start != 0 {
                return Err("empty status buffer has a nonzero offset".into());
            }
        } else {
            let end = start.checked_add(length).ok_or("status buffer overflow")?;
            if start < 20 || msg.get(start..end).is_none() {
                return Err("status buffer is outside the indication".into());
            }
        }
        Ok(match wire::u32_at(msg, 8)? {
            0x4001_000b => Self::MediaConnect,
            0x4001_000c => Self::MediaDisconnect,
            _ => Self::Other,
        })
    }
}

struct Message;
impl Message {
    fn parse(bytes: &[u8], kind: u32, minimum: usize) -> Result<&[u8]> {
        if bytes.len() > TRANSFER_LIMIT || bytes.len() < minimum {
            return Err("invalid control response size".into());
        }
        let length = wire::u32_at(bytes, 4)? as usize;
        if length < minimum || length > bytes.len() || wire::u32_at(bytes, 0)? != kind {
            return Err("invalid control message type or length".into());
        }
        Ok(&bytes[..length])
    }
}
