//! Amiga Disk File reader supporting OFS and FFS volumes (DD and HD images).

use super::{be32, latin1};
use std::collections::HashSet;
use std::fmt;

const BSIZE: usize = 512;
const HASH_SIZE: usize = 72;
const T_HEADER: u32 = 2;
const T_LIST: u32 = 16;
const ST_ROOT: i32 = 1;
const ST_USERDIR: i32 = 2;
const ST_FILE: i32 = -3;

#[derive(Debug)]
pub enum AdfError {
    NotDos,
    BadBlock(u32),
    Corrupt(&'static str),
}

impl fmt::Display for AdfError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AdfError::NotDos => write!(f, "not an AmigaDOS disk image"),
            AdfError::BadBlock(b) => write!(f, "block {b} is outside the image"),
            AdfError::Corrupt(why) => write!(f, "corrupt filesystem: {why}"),
        }
    }
}

impl std::error::Error for AdfError {}

pub struct AdfFile {
    /// Path inside the volume, `/`-separated.
    pub path: String,
    pub data: Vec<u8>,
}

pub struct Adf {
    data: Vec<u8>,
    ffs: bool,
    root: u32,
    pub volume: String,
}

impl Adf {
    pub fn open(data: Vec<u8>) -> Result<Self, AdfError> {
        if data.len() < 4 * BSIZE || &data[0..3] != b"DOS" {
            return Err(AdfError::NotDos);
        }
        let ffs = data[3] & 1 != 0;
        let root = (data.len() / BSIZE / 2) as u32;
        let mut adf = Adf {
            data,
            ffs,
            root,
            volume: String::new(),
        };
        let rb = adf.block(root)?;
        if be32(rb, 0) != T_HEADER || be32(rb, BSIZE - 4) as i32 != ST_ROOT {
            return Err(AdfError::Corrupt("root block has the wrong type"));
        }
        adf.volume = name_of(rb);
        Ok(adf)
    }

    fn block(&self, n: u32) -> Result<&[u8], AdfError> {
        let start = n as usize * BSIZE;
        self.data
            .get(start..start + BSIZE)
            .ok_or(AdfError::BadBlock(n))
    }

    /// Every regular file on the volume, depth first.
    pub fn files(&self) -> Result<Vec<AdfFile>, AdfError> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk_dir(self.root, "", &mut out, &mut seen, 0)?;
        Ok(out)
    }

    fn walk_dir(
        &self,
        dir: u32,
        prefix: &str,
        out: &mut Vec<AdfFile>,
        seen: &mut HashSet<u32>,
        depth: usize,
    ) -> Result<(), AdfError> {
        if depth > 32 {
            return Err(AdfError::Corrupt("directory nesting too deep"));
        }
        let db = self.block(dir)?;
        for slot in 0..HASH_SIZE {
            let mut entry = be32(db, 24 + slot * 4);
            while entry != 0 {
                if !seen.insert(entry) {
                    return Err(AdfError::Corrupt("loop in hash chain"));
                }
                let hb = self.block(entry)?;
                if be32(hb, 0) != T_HEADER {
                    return Err(AdfError::Corrupt("hash chain points at a non-header block"));
                }
                let name = name_of(hb);
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                match be32(hb, BSIZE - 4) as i32 {
                    ST_USERDIR => self.walk_dir(entry, &path, out, seen, depth + 1)?,
                    ST_FILE => out.push(AdfFile {
                        data: self.read_file(entry)?,
                        path,
                    }),
                    _ => {} // links and other oddities are not used by the game disks
                }
                entry = be32(hb, BSIZE - 16);
            }
        }
        Ok(())
    }

    fn read_file(&self, header: u32) -> Result<Vec<u8>, AdfError> {
        let hb = self.block(header)?;
        let size = be32(hb, 324) as usize;
        if size > self.data.len() {
            return Err(AdfError::Corrupt("file larger than the disk"));
        }
        // Data block pointers are stored in reverse in each header/extension
        // block: slot 71 is the first block.
        let mut blocks = Vec::new();
        let mut cur = header;
        let mut hops = 0;
        loop {
            let b = self.block(cur)?;
            if cur != header && be32(b, 0) != T_LIST {
                return Err(AdfError::Corrupt("bad file extension block"));
            }
            let count = (be32(b, 8) as usize).min(HASH_SIZE);
            for i in 0..count {
                blocks.push(be32(b, 24 + (HASH_SIZE - 1 - i) * 4));
            }
            cur = be32(b, BSIZE - 8);
            hops += 1;
            if cur == 0 || hops > self.data.len() / BSIZE {
                break;
            }
        }

        let mut data = Vec::with_capacity(size);
        for blk in blocks {
            if data.len() >= size {
                break;
            }
            let b = self.block(blk)?;
            if self.ffs {
                data.extend_from_slice(b);
            } else {
                let n = (be32(b, 12) as usize).min(BSIZE - 24);
                data.extend_from_slice(&b[24..24 + n]);
            }
        }
        if data.len() < size {
            return Err(AdfError::Corrupt(
                "file data shorter than its header claims",
            ));
        }
        data.truncate(size);
        Ok(data)
    }
}

fn name_of(block: &[u8]) -> String {
    let len = (block[BSIZE - 80] as usize).min(30);
    latin1(&block[BSIZE - 79..BSIZE - 79 + len])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal one-file OFS DD image to exercise the reader.
    fn tiny_ofs(contents: &[u8]) -> Vec<u8> {
        let mut img = vec![0u8; 880 * 2 * BSIZE];
        img[0..4].copy_from_slice(b"DOS\0");
        let put = |img: &mut Vec<u8>, blk: usize, off: usize, v: u32| {
            img[blk * BSIZE + off..blk * BSIZE + off + 4].copy_from_slice(&v.to_be_bytes());
        };
        let root = 880;
        put(&mut img, root, 0, T_HEADER);
        put(&mut img, root, BSIZE - 4, ST_ROOT as u32);
        let fh = 882;
        // Hash slot is irrelevant to the reader; use slot 0.
        put(&mut img, root, 24, fh as u32);
        put(&mut img, fh, 0, T_HEADER);
        put(&mut img, fh, BSIZE - 4, ST_FILE as u32);
        let name = b"hello";
        img[fh * BSIZE + BSIZE - 80] = name.len() as u8;
        img[fh * BSIZE + BSIZE - 79..fh * BSIZE + BSIZE - 79 + name.len()].copy_from_slice(name);
        put(&mut img, fh, 324, contents.len() as u32);
        let chunks: Vec<&[u8]> = contents.chunks(BSIZE - 24).collect();
        put(&mut img, fh, 8, chunks.len() as u32);
        for (i, chunk) in chunks.iter().enumerate() {
            let db = 883 + i;
            put(&mut img, fh, 24 + (HASH_SIZE - 1 - i) * 4, db as u32);
            put(&mut img, db, 12, chunk.len() as u32);
            img[db * BSIZE + 24..db * BSIZE + 24 + chunk.len()].copy_from_slice(chunk);
        }
        img
    }

    #[test]
    fn reads_ofs_file() {
        let contents: Vec<u8> = (0..1500u32).map(|i| (i * 7) as u8).collect();
        let adf = Adf::open(tiny_ofs(&contents)).unwrap();
        let files = adf.files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "hello");
        assert_eq!(files[0].data, contents);
    }

    #[test]
    fn rejects_non_dos() {
        assert!(matches!(Adf::open(vec![0; 901120]), Err(AdfError::NotDos)));
    }
}
