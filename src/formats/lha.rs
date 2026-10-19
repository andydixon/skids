//! LHA/LZH archive reader (header levels 0, 1 and 2).
//!
//! Supports the stored (`-lh0-`, `-lz4-`) and LZSS/Huffman (`-lh5-`, `-lh6-`,
//! `-lh7-`) methods. Every decoded file is checked against the header CRC.
//! Entries in other methods, or that fail to decode, carry the reason in
//! `data` instead of their contents.

use super::lzh;
use super::{latin1, le16, le32};
use std::fmt;

#[derive(Debug)]
pub enum LhaError {
    Truncated(usize),
    BadHeader(usize),
}

impl fmt::Display for LhaError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LhaError::Truncated(at) => write!(f, "archive truncated at byte {at}"),
            LhaError::BadHeader(at) => write!(f, "unrecognised LHA header at byte {at}"),
        }
    }
}

impl std::error::Error for LhaError {}

pub struct LhaEntry {
    /// Path inside the archive, `/`-separated.
    pub path: String,
    pub method: String,
    /// File contents, or why they could not be read.
    pub data: Result<Vec<u8>, String>,
}

pub fn parse(d: &[u8]) -> Result<Vec<LhaEntry>, LhaError> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < d.len() && d[p] != 0 {
        if p + 22 > d.len() {
            return Err(LhaError::Truncated(p));
        }
        let method = &d[p + 2..p + 7];
        if method[0] != b'-' || method[4] != b'-' {
            return Err(LhaError::BadHeader(p));
        }
        let method = latin1(method);
        let mut packed = le32(d, p + 7) as usize;
        let original = le32(d, p + 11) as usize;
        let level = d[p + 20];
        let mut crc = None;

        let mut name = String::new();
        let mut dir = String::new();
        let data_start;

        match level {
            0 | 1 => {
                let header_len = d[p] as usize + 2;
                let name_len = d[p + 21] as usize;
                if p + 22 + name_len > d.len() || p + header_len > d.len() {
                    return Err(LhaError::Truncated(p));
                }
                name = latin1(&d[p + 22..p + 22 + name_len]);
                if p + 24 + name_len <= d.len() {
                    crc = Some(le16(d, p + 22 + name_len));
                }
                let mut q = p + header_len;
                if level == 1 {
                    // Extended headers follow the base header and are counted
                    // in the packed size.
                    let mut next = le16(d, q - 2) as usize;
                    while next != 0 {
                        if q + next > d.len() || next < 3 {
                            return Err(LhaError::Truncated(q));
                        }
                        read_ext(&d[q..q + next - 2], &mut name, &mut dir);
                        packed = packed.checked_sub(next).ok_or(LhaError::BadHeader(q))?;
                        q += next;
                        next = le16(d, q - 2) as usize;
                    }
                }
                data_start = q;
            }
            2 => {
                let header_len = le16(d, p) as usize;
                if p + header_len > d.len() || header_len < 26 {
                    return Err(LhaError::Truncated(p));
                }
                crc = Some(le16(d, p + 21));
                let mut q = p + 26;
                let mut next = le16(d, p + 24) as usize;
                while next != 0 {
                    if q + next > p + header_len || next < 3 {
                        return Err(LhaError::BadHeader(q));
                    }
                    read_ext(&d[q..q + next - 2], &mut name, &mut dir);
                    q += next;
                    next = le16(d, q - 2) as usize;
                }
                data_start = p + header_len;
            }
            _ => return Err(LhaError::BadHeader(p)),
        }

        let data_end = data_start + packed;
        if data_end > d.len() {
            return Err(LhaError::Truncated(data_start));
        }
        let path = join_path(&dir, &name);
        let body = &d[data_start..data_end];
        let decoded = match method.as_str() {
            "-lh0-" | "-lz4-" => Ok(body.to_vec()),
            "-lh5-" => lzh::decode(body, original, 13).map_err(|e| e.to_string()),
            "-lh6-" => lzh::decode(body, original, 15).map_err(|e| e.to_string()),
            "-lh7-" => lzh::decode(body, original, 16).map_err(|e| e.to_string()),
            other => Err(format!("unsupported method {other}")),
        };
        let data = decoded.and_then(|v| match crc {
            Some(c) if lzh::crc16(&v) != c => Err("CRC mismatch".to_string()),
            _ => Ok(v),
        });
        if method != "-lhd-" {
            out.push(LhaEntry { path, method, data });
        }
        p = data_end;
    }
    Ok(out)
}

fn read_ext(ext: &[u8], name: &mut String, dir: &mut String) {
    match ext.first() {
        Some(1) => *name = latin1(&ext[1..]),
        Some(2) => *dir = latin1(&ext[1..]),
        _ => {}
    }
}

fn join_path(dir: &str, name: &str) -> String {
    let norm = |s: &str| -> String {
        s.chars()
            .map(|c| if c == '\\' || c == '\u{ff}' { '/' } else { c })
            .collect::<String>()
            .trim_matches('/')
            .to_string()
    };
    let (dir, name) = (norm(dir), norm(name));
    if dir.is_empty() {
        name
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level0_entry(name: &str, body: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 22];
        h[2..7].copy_from_slice(b"-lh0-");
        h[7..11].copy_from_slice(&(body.len() as u32).to_le_bytes());
        h[11..15].copy_from_slice(&(body.len() as u32).to_le_bytes());
        h[20] = 0;
        h[21] = name.len() as u8;
        h.extend_from_slice(name.as_bytes());
        h.extend_from_slice(&lzh::crc16(body).to_le_bytes());
        h[0] = (h.len() - 2) as u8;
        h.extend_from_slice(body);
        h
    }

    #[test]
    fn parses_stored_level0() {
        let mut arc = level0_entry("dir\\a.bin", b"hello");
        arc.extend(level0_entry("b", b"world!"));
        arc.push(0);
        let e = parse(&arc).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "dir/a.bin");
        assert_eq!(e[0].data.as_deref(), Ok(&b"hello"[..]));
        assert_eq!(e[1].data.as_deref(), Ok(&b"world!"[..]));
    }
}
