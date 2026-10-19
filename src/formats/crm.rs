//! CrunchMania decruncher (`CrM!`, `CrM2` and the delta-sampled `Crm!`/`Crm2`).
//!
//! Header: 4-byte id, 2 unused bytes, BE32 unpacked size, BE32 packed size.
//! The packed stream is read backwards, byte by byte, LSB first, and the
//! output is also written from the end towards the start. The final six
//! bytes of the packed data seed the bit buffer.
//!
//! The bitstream layout follows the description in Teemu Suutari's
//! "ancient" decompression library (BSD-2-Clause).

use super::{be16, be32};
use std::fmt;

#[derive(Debug, PartialEq)]
pub enum CrmError {
    NotCrunched,
    Truncated,
    BadHuffman,
    BadReference,
    SizeMismatch,
}

impl fmt::Display for CrmError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            CrmError::NotCrunched => "not CrunchMania data",
            CrmError::Truncated => "packed stream ended early",
            CrmError::BadHuffman => "invalid Huffman table",
            CrmError::BadReference => "back-reference outside the output",
            CrmError::SizeMismatch => "output size does not match the header",
        };
        f.write_str(s)
    }
}

impl std::error::Error for CrmError {}

pub fn is_crunched(d: &[u8]) -> bool {
    d.len() >= 20 && matches!(&d[0..4], b"CrM!" | b"CrM2" | b"Crm!" | b"Crm2")
}

struct BackBits<'a> {
    d: &'a [u8],
    start: usize,
    pos: usize,
    buf: u32,
    len: u32,
}

impl BackBits<'_> {
    #[inline]
    fn bits(&mut self, mut count: u32) -> Result<u32, CrmError> {
        let mut ret = 0u32;
        let mut shift = 0u32;
        while count > 0 {
            if self.len == 0 {
                if self.pos <= self.start {
                    return Err(CrmError::Truncated);
                }
                self.pos -= 1;
                self.buf = self.d[self.pos] as u32;
                self.len = 8;
            }
            let take = count.min(self.len);
            ret |= (self.buf & ((1 << take) - 1)) << shift;
            self.buf >>= take;
            self.len -= take;
            count -= take;
            shift += take;
        }
        Ok(ret)
    }

    #[inline]
    fn bit(&mut self) -> Result<u32, CrmError> {
        self.bits(1)
    }
}

/// Canonical-style decoder: codes of each depth are consecutive.
struct Huffman {
    max_depth: u32,
    first: [u32; 16],
    count: [u32; 16],
    offset: [usize; 16],
    values: Vec<u32>,
}

impl Huffman {
    fn read(br: &mut BackBits, code_len: u32) -> Result<Self, CrmError> {
        let max_depth = br.bits(4)?;
        if max_depth == 0 {
            return Err(CrmError::BadHuffman);
        }
        let mut lengths = [0u32; 16];
        for i in 0..max_depth {
            lengths[i as usize] = br.bits((i + 1).min(code_len))?;
        }
        let mut h = Huffman {
            max_depth,
            first: [0; 16],
            count: [0; 16],
            offset: [0; 16],
            values: Vec::new(),
        };
        let mut code = 0u32;
        for depth in 1..=max_depth {
            let di = depth as usize;
            h.first[di] = code >> (max_depth - depth);
            h.count[di] = lengths[di - 1];
            h.offset[di] = h.values.len();
            for _ in 0..lengths[di - 1] {
                h.values.push(br.bits(code_len)?);
                code += 1 << (max_depth - depth);
            }
            if code > (1 << max_depth) {
                return Err(CrmError::BadHuffman);
            }
        }
        Ok(h)
    }

    #[inline]
    fn decode(&self, br: &mut BackBits) -> Result<u32, CrmError> {
        let mut code = 0u32;
        for depth in 1..=self.max_depth as usize {
            code = (code << 1) | br.bit()?;
            let rel = code.wrapping_sub(self.first[depth]);
            if rel < self.count[depth] {
                return Ok(self.values[self.offset[depth] + rel as usize]);
            }
        }
        Err(CrmError::BadHuffman)
    }
}

pub fn decrunch(d: &[u8]) -> Result<Vec<u8>, CrmError> {
    if !is_crunched(d) {
        return Err(CrmError::NotCrunched);
    }
    let lzh = d[3] == b'2';
    let sampled = d[2] == b'm';
    let raw = be32(d, 6) as usize;
    let packed = be32(d, 10) as usize;
    if packed < 6 || packed + 14 > d.len() || raw == 0 || raw > 64 << 20 {
        return Err(CrmError::Truncated);
    }
    let tail = packed + 14 - 6;
    let shift = be16(d, tail + 4) as u32;
    if shift > 16 {
        return Err(CrmError::Truncated);
    }
    let mut br = BackBits {
        d,
        start: 14,
        pos: tail,
        buf: be32(d, tail) >> (16 - shift),
        len: shift + 16,
    };

    let mut out = vec![0u8; raw];
    let mut o = raw;

    macro_rules! put {
        ($b:expr) => {{
            if o == 0 {
                return Err(CrmError::SizeMismatch);
            }
            o -= 1;
            out[o] = $b;
        }};
    }
    macro_rules! copy {
        ($dist:expr, $count:expr) => {{
            let (dist, count) = ($dist as usize, $count as usize);
            if dist == 0 || count > o || o + dist > raw {
                return Err(CrmError::BadReference);
            }
            for _ in 0..count {
                o -= 1;
                out[o] = out[o + dist];
            }
        }};
    }

    if lzh {
        loop {
            let lengths = Huffman::read(&mut br, 9)?;
            let distances = Huffman::read(&mut br, 4)?;
            let items = br.bits(16)? + 1;
            for _ in 0..items {
                let sym = lengths.decode(&mut br)?;
                if sym & 0x100 != 0 {
                    put!(sym as u8);
                } else {
                    let count = sym + 3;
                    let dbits = distances.decode(&mut br)?;
                    let dist = if dbits == 0 {
                        br.bits(1)? + 1
                    } else {
                        (br.bits(dbits)? | (1 << dbits)) + 1
                    };
                    copy!(dist, count);
                }
            }
            if br.bit()? == 0 {
                break;
            }
        }
    } else {
        const LEN_BASE: [u32; 4] = [0, 2, 6, 22];
        const LEN_BITS: [u32; 4] = [1, 2, 4, 8];
        const DIST_BASE: [u32; 3] = [0, 32, 544];
        const DIST_BITS: [u32; 3] = [5, 9, 14];
        while o > 0 {
            if br.bit()? != 0 {
                put!(br.bits(8)? as u8);
                continue;
            }
            // Length class: 0, 10, 110, 111
            let li = if br.bit()? == 0 {
                0
            } else if br.bit()? == 0 {
                1
            } else if br.bit()? == 0 {
                2
            } else {
                3
            };
            let mut count = LEN_BASE[li] + br.bits(LEN_BITS[li])? + 2;
            if count == 23 {
                // Literal run
                let run = if br.bit()? != 0 {
                    br.bits(5)? + 15
                } else {
                    br.bits(14)? + 15
                };
                for _ in 0..run {
                    put!(br.bits(8)? as u8);
                }
                continue;
            }
            if count > 23 {
                count -= 1;
            }
            // Distance class: 0 -> 1, 10 -> 0, 11 -> 2
            let di = if br.bit()? == 0 {
                1
            } else if br.bit()? == 0 {
                0
            } else {
                2
            };
            let dist = DIST_BASE[di] + br.bits(DIST_BITS[di])?;
            copy!(dist, count);
        }
    }

    if o != 0 {
        return Err(CrmError::SizeMismatch);
    }
    if sampled {
        let mut acc = 0u8;
        for b in out.iter_mut() {
            acc = acc.wrapping_add(*b);
            *b = acc;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_plain_data() {
        assert_eq!(decrunch(&[0u8; 64]), Err(CrmError::NotCrunched));
    }

    #[test]
    fn rejects_truncated_header() {
        let mut d = b"CrM2\0\0".to_vec();
        d.extend_from_slice(&100u32.to_be_bytes());
        d.extend_from_slice(&1000u32.to_be_bytes());
        d.extend_from_slice(&[0; 16]);
        assert_eq!(decrunch(&d), Err(CrmError::Truncated));
    }
}
