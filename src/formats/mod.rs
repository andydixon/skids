//! Readers for the Amiga container formats the game ships in.
//!
//! The original release is a set of ADF floppy images. Each image holds one
//! LHA archive (stored, no compression), and the files inside those archives
//! are individually packed with CrunchMania.

pub mod adf;
pub mod crm;
pub mod lha;

pub(crate) fn be16(d: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([d[off], d[off + 1]])
}

pub(crate) fn be32(d: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

pub(crate) fn le16(d: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([d[off], d[off + 1]])
}

pub(crate) fn le32(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

/// Amiga filenames are ISO-8859-1; map each byte straight to its code point.
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}
