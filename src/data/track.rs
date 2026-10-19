//! Track files (`track/TRACKn`).
//!
//! File layout:
//!   0x00  7 x BE32 end offsets. Section 0 runs from 0x1c to the first offset
//!         and holds the selector thumbnails; sections 1..6 are tracks.
//!
//! Each track section (281240 bytes once decrunched):
//!   0x0000  height map, 64x64 BE16 8.8 fixed point, one cell per 16x8 pixels
//!   0x2000  8192 bytes, unused (always zero on the retail disks)
//!   0x4000  BE16 gate count, start x, start y (screen pixels), flags
//!   0x4008  gates: x1 y1 x2 y2 BE16, world coordinates in 1/32 units
//!   0x4A98  four 1024x512 bitplanes (65536 bytes each), giving 16 colours
//!
//! The world is an isometric ground plane rotated 45 degrees to the picture:
//! screen x = wx - wy + 512, screen y = (wx + wy) * 31/64 - 252. The constants
//! were fitted so that 96% of gate samples over all 24 tracks land on road
//! pixels (against 40-70% for a plain 2:1 squash). The stored start point
//! only lands on the road on 14 of the 24 tracks, so the grid is placed from
//! the gates instead.

use super::palette::{self, Rgb};
use super::unpack;
use crate::formats::{be16, be32};

pub const W: usize = 1024;
pub const H: usize = 512;
pub const HM_W: usize = 64;
pub const HM_H: usize = 64;
pub const CELL_W: f32 = (W / HM_W) as f32;
pub const CELL_H: f32 = (H / HM_H) as f32;
const SECTION_SIZE: usize = 0x44a98;
const GATES_AT: usize = 0x4000;
const PLANES_AT: usize = 0x4a98;
const PLANE_SIZE: usize = W * H / 8;
const SECTIONS: usize = 7;
const ISO_X0: f32 = 512.0;
const ISO_Y_SCALE: f32 = 31.0 / 64.0;
const ISO_Y0: f32 = -252.0;

pub fn to_screen(wx: f32, wy: f32) -> (f32, f32) {
    (wx - wy + ISO_X0, (wx + wy) * ISO_Y_SCALE + ISO_Y0)
}

pub fn on_screen(sx: f32, sy: f32, margin: f32) -> bool {
    sx >= margin && sy >= margin && sx < W as f32 - margin && sy < H as f32 - margin
}

pub struct TrackFile {
    pub name: String,
    pub number: u32,
    /// Packed track sections (index 0 = first playable track).
    pub sections: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Gate {
    pub a: (f32, f32),
    pub b: (f32, f32),
}

impl Gate {
    pub fn mid(&self) -> (f32, f32) {
        ((self.a.0 + self.b.0) * 0.5, (self.a.1 + self.b.1) * 0.5)
    }
}

pub struct Track {
    pub name: String,
    /// Palette indices, W x H.
    pub pixels: Vec<u8>,
    pub palette: [Rgb; 16],
    /// Height samples in 8.8 fixed point.
    pub height: Vec<u16>,
    /// Gates in world coordinates.
    pub gates: Vec<Gate>,
    /// Start position and flags as stored. Kept for reference: the start
    /// point does not line up with the road reliably, and the flags' meaning
    /// is unknown.
    #[allow(dead_code)]
    pub start: (f32, f32),
    #[allow(dead_code)]
    pub flags: u16,
}

impl TrackFile {
    pub fn parse(name: &str, d: &[u8]) -> Result<TrackFile, String> {
        if d.len() < SECTIONS * 4 {
            return Err("file too short".into());
        }
        let ends: Vec<usize> = (0..SECTIONS).map(|i| be32(d, i * 4) as usize).collect();
        let mut start = SECTIONS * 4;
        let mut sections = Vec::new();
        for (i, &end) in ends.iter().enumerate() {
            if end < start || end > d.len() {
                return Err(format!("section {i} has a bad offset"));
            }
            if i > 0 {
                sections.push(d[start..end].to_vec());
            }
            start = end;
        }
        let number = name
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .unwrap_or(u32::MAX);
        Ok(TrackFile {
            name: name.to_string(),
            number,
            sections,
        })
    }

    pub fn track_name(&self, section: usize) -> String {
        format!("{}-{}", self.name.to_uppercase(), section + 1)
    }

    pub fn decode(&self, section: usize) -> Result<Track, String> {
        let raw = unpack(&self.sections[section]).map_err(|e| e.to_string())?;
        Track::from_section(self.track_name(section), &raw)
    }
}

impl Track {
    pub fn from_section(name: String, d: &[u8]) -> Result<Track, String> {
        if d.len() < SECTION_SIZE {
            return Err(format!(
                "track section is {} bytes, expected {SECTION_SIZE}",
                d.len()
            ));
        }
        let height = (0..HM_W * HM_H).map(|i| be16(d, i * 2)).collect();

        let count = be16(d, GATES_AT) as usize;
        let start = (be16(d, GATES_AT + 2) as f32, be16(d, GATES_AT + 4) as f32);
        let flags = be16(d, GATES_AT + 6);
        if count == 0 || GATES_AT + 8 + count * 8 > PLANES_AT {
            return Err(format!("implausible gate count {count}"));
        }
        let gates = (0..count)
            .map(|i| {
                let o = GATES_AT + 8 + i * 8;
                let w = |k: usize| be16(d, o + k * 2) as f32 / 32.0;
                Gate {
                    a: (w(0), w(1)),
                    b: (w(2), w(3)),
                }
            })
            .collect();

        let planes = &d[PLANES_AT..PLANES_AT + 4 * PLANE_SIZE];
        let mut pixels = vec![0u8; W * H];
        for (p, plane) in planes.chunks_exact(PLANE_SIZE).enumerate() {
            for (byte_i, &byte) in plane.iter().enumerate() {
                if byte == 0 {
                    continue;
                }
                let base = byte_i * 8;
                for bit in 0..8 {
                    if byte & (0x80 >> bit) != 0 {
                        pixels[base + bit] |= 1 << p;
                    }
                }
            }
        }

        Ok(Track {
            name,
            pixels,
            palette: palette::TRACK_DEFAULT,
            height,
            gates,
            start,
            flags,
        })
    }

    /// Ground height (in height units) at a screen position, bilinear.
    pub fn height_at(&self, sx: f32, sy: f32) -> f32 {
        let fx = (sx / CELL_W - 0.5).clamp(0.0, (HM_W - 1) as f32);
        let fy = (sy / CELL_H - 0.5).clamp(0.0, (HM_H - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(HM_W - 1), (y0 + 1).min(HM_H - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let h = |x: usize, y: usize| self.height[y * HM_W + x] as f32 / 256.0;
        let top = h(x0, y0) * (1.0 - tx) + h(x1, y0) * tx;
        let bot = h(x0, y1) * (1.0 - tx) + h(x1, y1) * tx;
        top * (1.0 - ty) + bot * ty
    }

    /// Height at a world position.
    pub fn height_world(&self, wx: f32, wy: f32) -> f32 {
        let (sx, sy) = to_screen(wx, wy);
        self.height_at(sx, sy)
    }

    pub fn pixel(&self, sx: f32, sy: f32) -> u8 {
        let x = (sx as isize).clamp(0, W as isize - 1) as usize;
        let y = (sy as isize).clamp(0, H as isize - 1) as usize;
        self.pixels[y * W + x]
    }

    /// The upper half of the palette is used for the driving surface: the
    /// fourth bitplane traces the road on every retail track.
    pub fn on_road(&self, sx: f32, sy: f32) -> bool {
        self.pixel(sx, sy) & 8 != 0
    }

    pub fn on_road_world(&self, wx: f32, wy: f32) -> bool {
        let (sx, sy) = to_screen(wx, wy);
        on_screen(sx, sy, 0.0) && self.on_road(sx, sy)
    }

    pub fn rgb(&self) -> Vec<u32> {
        let pal: Vec<u32> = self.palette.iter().map(|c| c.to_u32()).collect();
        self.pixels.iter().map(|&i| pal[i as usize]).collect()
    }
}
