//! OctaMED module reader (MMD0 and MMD1, 8-bit sample instruments).
//!
//! Layout notes (all offsets big-endian, pointers relative to file start):
//!   0  "MMD0"/"MMD1", 8 song struct, 16 block table, 24 sample table.
//! Song struct: 63 x 8-byte sample entries (repeat, repeat length in words,
//! MIDI channel, preset, volume, transpose), then block count, sequence
//! length, 256-byte play sequence, default tempo, transpose, flags, flags2,
//! secondary tempo (ticks per line), 16 track volumes, master volume,
//! sample count. MMD0 blocks have a 2-byte header and 3-byte notes; MMD1
//! blocks have an 8-byte header and 4-byte notes.

use super::{be16, be32};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Note {
    /// 0 = none, 1 = C-1 ...
    pub note: u8,
    /// 0 = none, else instrument number (1-based).
    pub instr: u8,
    pub cmd: u8,
    pub param: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Instrument {
    pub data: Vec<i8>,
    pub loop_start: usize,
    pub loop_len: usize,
    pub volume: u8,
    pub transpose: i8,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub tracks: usize,
    pub lines: usize,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub instruments: Vec<Instrument>,
    pub blocks: Vec<Block>,
    pub sequence: Vec<usize>,
    /// Lines per minute is derived from these by the player.
    pub tempo: u16,
    pub speed: u8,
    pub bpm_mode: bool,
    pub lines_per_beat: u8,
    pub volume_hex: bool,
    pub transpose: i8,
    pub track_volume: [u8; 16],
    pub master_volume: u8,
}

pub fn is_med(d: &[u8]) -> bool {
    d.len() > 52 && (&d[0..4] == b"MMD0" || &d[0..4] == b"MMD1")
}

pub fn parse(d: &[u8]) -> Result<Song, String> {
    if !is_med(d) {
        return Err("not an OctaMED MMD0/MMD1 module".into());
    }
    let mmd1 = d[3] == b'1';
    let get = |o: usize, n: usize| {
        d.get(o..o + n)
            .ok_or_else(|| format!("offset {o} is outside the module"))
    };
    let song = be32(d, 8) as usize;
    let blockarr = be32(d, 16) as usize;
    let smplarr = be32(d, 24) as usize;
    let s = get(song, 788)?;
    let nblocks = be16(s, 504) as usize;
    let songlen = (be16(s, 506) as usize).min(256);
    let sequence: Vec<usize> = s[508..508 + songlen].iter().map(|&b| b as usize).collect();
    let tempo = be16(s, 764);
    let transpose = s[766] as i8;
    let flags = s[767];
    let flags2 = s[768];
    let speed = s[769].max(1);
    let mut track_volume = [64u8; 16];
    track_volume.copy_from_slice(&s[770..786]);
    let master_volume = s[786];
    let nsamples = (s[787] as usize).min(63);

    let mut instruments = Vec::new();
    for i in 0..nsamples {
        let e = &s[i * 8..i * 8 + 8];
        let (rep, replen) = (be16(e, 0) as usize * 2, be16(e, 2) as usize * 2);
        let mut inst = Instrument {
            volume: e[6].min(64),
            transpose: e[7] as i8,
            ..Default::default()
        };
        let p = if smplarr != 0 {
            be32(get(smplarr + i * 4, 4)?, 0) as usize
        } else {
            0
        };
        if p != 0 {
            let h = get(p, 6)?;
            let len = be32(h, 0) as usize;
            let kind = be16(h, 4) as i16;
            if kind != 0 {
                return Err(format!("instrument {} is a synth or multi-octave sample (type {kind}), which is not supported", i + 1));
            }
            inst.data = get(p + 6, len)?.iter().map(|&b| b as i8).collect();
            if replen > 2 && rep < len {
                inst.loop_start = rep;
                inst.loop_len = replen.min(len - rep);
            }
        }
        instruments.push(inst);
    }

    let mut blocks = Vec::new();
    for b in 0..nblocks {
        let bp = be32(get(blockarr + b * 4, 4)?, 0) as usize;
        let (tracks, lines, header, width) = if mmd1 {
            let h = get(bp, 8)?;
            (be16(h, 0) as usize, be16(h, 2) as usize + 1, 8, 4)
        } else {
            let h = get(bp, 2)?;
            (h[0] as usize, h[1] as usize + 1, 2, 3)
        };
        if tracks == 0 || tracks > 64 {
            return Err(format!("block {b} has {tracks} tracks"));
        }
        let body = get(bp + header, tracks * lines * width)?;
        let notes = body
            .chunks_exact(width)
            .map(|c| {
                if mmd1 {
                    Note {
                        note: c[0] & 0x7f,
                        instr: c[1] & 0x3f,
                        cmd: c[2],
                        param: c[3],
                    }
                } else {
                    // MMD0: nnnnnnn i | iiii cccc | pppppppp
                    let instr = ((c[0] & 0x80) >> 3) | (c[1] >> 4);
                    Note {
                        note: c[0] & 0x3f,
                        instr,
                        cmd: c[1] & 0x0f,
                        param: c[2],
                    }
                }
            })
            .collect();
        blocks.push(Block {
            tracks,
            lines,
            notes,
        });
    }
    if sequence.iter().any(|&b| b >= blocks.len()) || sequence.is_empty() {
        return Err("play sequence refers to a missing block".into());
    }
    Ok(Song {
        instruments,
        blocks,
        sequence,
        tempo,
        speed,
        bpm_mode: flags2 & 0x20 != 0,
        lines_per_beat: (flags2 & 0x1f) + 1,
        volume_hex: flags & 0x10 != 0,
        transpose,
        track_volume,
        master_volume,
    })
}
