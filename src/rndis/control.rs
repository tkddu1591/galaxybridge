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
        if bytes.len() > TRANSFER_LIMIT || bytes.len() < 16 {
            return Err("invalid control response size".into());
        }
        let len = wire::u32_at(bytes, 4)? as usize;
        if len < 16 || len > bytes.len() {
            return Err("invalid control message length".into());
        }
        let msg = &bytes[..len];
        if wire::u32_at(msg, 0)? != (self.kind | 0x8000_0000) || wire::u32_at(msg, 8)? != self.id {
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
        if msg.len() < 52 {
            return Err("short initialize completion".into());
        }
        if wire::u32_at(msg, 16)? != 1
            || wire::u32_at(msg, 20)? != 0
            || wire::u32_at(msg, 24)? & 1 == 0
            || wire::u32_at(msg, 28)? != 0
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
        if msg.len() < 24 {
            return Err("short query completion".into());
        }
        let len = wire::u32_at(msg, 16)? as usize;
        if len == 0 {
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
