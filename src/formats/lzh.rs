//! Decoder for the LHA `-lh5-`, `-lh6-` and `-lh7-` methods: LZSS with
//! static Huffman blocks, as used by LHA 2.x on the Amiga.
//!
//! Each block starts with a 16-bit symbol count, then three code-length
//! tables: one for the code-length codes (19 symbols), one for literals and
//! lengths (510 symbols) and one for match offsets. Codes are canonical,
//! assigned in symbol order within each length, and read MSB first.

use std::fmt;

#[derive(Debug, PartialEq)]
pub enum LzhError {
    Truncated,
    BadTable,
    BadReference,
}

impl fmt::Display for LzhError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            LzhError::Truncated => "compressed data ended early",
            LzhError::BadTable => "invalid Huffman table",
            LzhError::BadReference => "match reaches before the start of the file",
        })
    }
}

impl std::error::Error for LzhError {}

const NC: usize = 510;
const NT: usize = 19;
const TBIT: u32 = 5;
const CBIT: u32 = 9;

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u64,
    len: u32,
}

impl Bits<'_> {
    fn fill(&mut self) {
        while self.len <= 56 {
            // Reading past the end yields zero bits; callers detect overrun
            // through `overrun()` once a block has been decoded.
            let b = self.d.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.buf |= (b as u64) << (56 - self.len);
            self.len += 8;
        }
    }

    fn peek(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.fill();
        (self.buf >> (64 - n)) as u32
    }

    fn skip(&mut self, n: u32) {
        self.fill();
        self.buf <<= n;
        self.len -= n;
    }

    fn get(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.skip(n);
        v
    }

    fn overrun(&self) -> bool {
        self.pos > self.d.len() + 8
    }
}

/// Canonical Huffman decoder built from code lengths.
struct Table {
    /// A table with one symbol consumes no bits.
    single: Option<u16>,
    count: [u16; 17],
    symbols: Vec<u16>,
}

impl Table {
    fn single(sym: u16) -> Table {
        Table {
            single: Some(sym),
            count: [0; 17],
            symbols: Vec::new(),
        }
    }

    fn from_lengths(lengths: &[u8]) -> Result<Table, LzhError> {
        let mut count = [0u16; 17];
        for &l in lengths {
            if l as usize > 16 {
                return Err(LzhError::BadTable);
            }
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut symbols = Vec::new();
        for len in 1..=16u8 {
            for (s, &l) in lengths.iter().enumerate() {
                if l == len {
                    symbols.push(s as u16);
                }
            }
        }
        // Over-subscribed code sets are corrupt; incomplete ones are allowed.
        let mut left = 1i32;
        for &c in &count[1..] {
            left = left * 2 - c as i32;
            if left < 0 {
                return Err(LzhError::BadTable);
            }
        }
        Ok(Table {
            single: None,
            count,
            symbols,
        })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, LzhError> {
        if let Some(s) = self.single {
            return Ok(s);
        }
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..=16 {
            code |= bits.get(1) as i32;
            let n = self.count[len] as i32;
            if code - first < n {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += n;
            first = (first + n) << 1;
            code <<= 1;
        }
        Err(LzhError::BadTable)
    }
}

/// Reads the code-length or offset table (`read_pt_len` in LHA).
fn read_pt(
    bits: &mut Bits,
    nn: usize,
    nbit: u32,
    special: Option<usize>,
) -> Result<Table, LzhError> {
    let n = bits.get(nbit) as usize;
    if n == 0 {
        return Ok(Table::single(bits.get(nbit) as u16));
    }
    if n > nn {
        return Err(LzhError::BadTable);
    }
    let mut lens = vec![0u8; nn];
    let mut i = 0;
    while i < n {
        let mut c = bits.peek(3);
        if c == 7 {
            // Lengths of 7 and above: unary extension after the first 3 bits.
            let mut mask = 1u32 << 12;
            let look = bits.peek(16);
            while look & mask != 0 {
                mask >>= 1;
                c += 1;
                if c > 16 {
                    return Err(LzhError::BadTable);
                }
            }
            bits.skip(c - 3);
        } else {
            bits.skip(3);
        }
        lens[i] = c as u8;
        i += 1;
        if Some(i) == special {
            let zeros = bits.get(2) as usize;
            i = (i + zeros).min(nn);
        }
    }
    Table::from_lengths(&lens)
}

/// Reads the literal/length table (`read_c_len` in LHA).
fn read_c(bits: &mut Bits, pt: &Table) -> Result<Table, LzhError> {
    let n = bits.get(CBIT) as usize;
    if n == 0 {
        return Ok(Table::single(bits.get(CBIT) as u16));
    }
    if n > NC {
        return Err(LzhError::BadTable);
    }
    let mut lens = vec![0u8; NC];
    let mut i = 0;
    while i < n {
        match pt.decode(bits)? {
            0 => i += 1,
            1 => i += bits.get(4) as usize + 3,
            2 => i += bits.get(CBIT) as usize + 20,
            c => {
                lens[i] = (c - 2) as u8;
                i += 1;
            }
        }
    }
    if i > NC {
        return Err(LzhError::BadTable);
    }
    Table::from_lengths(&lens)
}

/// Decodes `original_size` bytes. `dict_bits` is 13 for lh5, 15 for lh6
/// and 16 for lh7.
pub fn decode(d: &[u8], original_size: usize, dict_bits: u32) -> Result<Vec<u8>, LzhError> {
    let np = dict_bits as usize + 1;
    let pbit = if dict_bits >= 15 { 5 } else { 4 };
    let mut bits = Bits {
        d,
        pos: 0,
        buf: 0,
        len: 0,
    };
    let mut out = Vec::with_capacity(original_size);
    while out.len() < original_size {
        let mut left = bits.get(16);
        if left == 0 {
            return Err(LzhError::Truncated);
        }
        let pt = read_pt(&mut bits, NT, TBIT, Some(3))?;
        let c = read_c(&mut bits, &pt)?;
        let p = read_pt(&mut bits, np, pbit, None)?;
        while left > 0 && out.len() < original_size {
            left -= 1;
            let sym = c.decode(&mut bits)? as usize;
            if sym < 256 {
                out.push(sym as u8);
                continue;
            }
            let length = sym - 256 + 3;
            let mut dist = p.decode(&mut bits)? as u32;
            if dist > 1 {
                dist = (1 << (dist - 1)) + bits.get(dist - 1);
            }
            let dist = dist as usize + 1;
            if dist > out.len() {
                return Err(LzhError::BadReference);
            }
            let start = out.len() - dist;
            for k in 0..length.min(original_size - out.len()) {
                out.push(out[start + k]);
            }
        }
        if bits.overrun() {
            return Err(LzhError::Truncated);
        }
    }
    Ok(out)
}

/// CRC-16 as used in LHA headers (polynomial 0xA001, reflected, init 0).
pub fn crc16(d: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in d {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xa001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_check_value() {
        // Standard check value for CRC-16/ARC.
        assert_eq!(crc16(b"123456789"), 0xbb3d);
    }

    #[test]
    fn canonical_decode() {
        // Lengths a=1, b=2, c=2 give codes 0, 10, 11.
        let t = Table::from_lengths(&[1, 2, 2]).unwrap();
        let data = [0b0101_1000u8];
        let mut bits = Bits {
            d: &data,
            pos: 0,
            buf: 0,
            len: 0,
        };
        assert_eq!(t.decode(&mut bits), Ok(0));
        assert_eq!(t.decode(&mut bits), Ok(1));
        assert_eq!(t.decode(&mut bits), Ok(2));
    }
}
