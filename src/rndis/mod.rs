//! Original protocol implementation from Microsoft's RNDIS message specifications.
//! No TetherKit source or binary is used.
pub mod control;
pub mod packet;

pub(crate) mod wire {
    use crate::Result;
    pub fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
        let end = offset.checked_add(4).ok_or("offset overflow")?;
        let word: [u8; 4] = bytes.get(offset..end).ok_or("truncated word")?.try_into()?;
        Ok(u32::from_le_bytes(word))
    }
    pub fn words(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
}
