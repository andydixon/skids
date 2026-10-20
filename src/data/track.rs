//! Track files (`track/TRACKn`).
//!
//! File layout:
//!   0x00  7 x BE32 end offsets. Section 0 runs from 0x1c to the first offset
//!         and holds the selector thumbnails; sections 1..6 are tracks.
//!         Section 0 starts with the selector thumbnails: a 640x64 16-colour
//!         picture, four bitplanes one after another (80 bytes per row), with
//!         the six tracks' 123x32 thumbnails in a 3x2 grid at its top left.
//!         Its pixels are Amiga hi-res, so each shows at 123x64 on screen.
//!         The last 142 bytes of section 0 are: BE16 file number, the 16
//!         colour palette shared by all six tracks as 48 BE16 values (R, G, B
//!         per colour, 0-15 each), 12 bytes of unknown settings, then the
//!         name of the set of tracks, NUL-terminated.
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

use super::image;
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

pub const THUMB_W: usize = 123;
pub const THUMB_H: usize = 32;

pub struct TrackFile {
    pub name: String,
    pub number: u32,
    pub palette: [Rgb; 16],
    pub set_name: String,
    /// The six selector thumbnails, THUMB_W x THUMB_H palette indices each.
    pub thumbnails: Vec<Vec<u8>>,
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
    /// The set of six tracks this one belongs to.
    pub set_name: String,
    /// The original selector thumbnail, THUMB_W x THUMB_H palette indices.
    pub thumbnail: Option<Vec<u8>>,
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
        let header = &d[SECTIONS * 4..ends[0]];
        let thumbnails = thumbnails(header);
        let (palette, set_name) =
            file_header(header).unwrap_or((palette::TRACK_DEFAULT, String::new()));
        Ok(TrackFile {
            name: name.to_string(),
            number,
            palette,
            set_name,
            thumbnails,
            sections,
        })
    }

    pub fn track_name(&self, section: usize) -> String {
        format!("{}-{}", self.name.to_uppercase(), section + 1)
    }

    pub fn decode(&self, section: usize) -> Result<Track, String> {
        let raw = unpack(&self.sections[section]).map_err(|e| e.to_string())?;
        let mut t = Track::from_section(self.track_name(section), &raw)?;
        t.palette = self.palette;
        t.set_name = self.set_name.clone();
        t.thumbnail = self.thumbnails.get(section).cloned();
        Ok(t)
    }
}

/// The six thumbnails from the start of section 0.
fn thumbnails(s: &[u8]) -> Vec<Vec<u8>> {
    const ROW: usize = 80;
    const ROWS: usize = 64;
    let plane = ROW * ROWS;
    if s.len() < plane * 4 {
        return Vec::new();
    }
    let px = |x: usize, y: usize| -> u8 {
        (0..4)
            .map(|p| ((s[p * plane + y * ROW + x / 8] >> (7 - x % 8)) & 1) << p)
            .sum()
    };
    (0..6)
        .map(|i| {
            let (ox, oy) = ((i % 3) * THUMB_W, (i / 3) * THUMB_H);
            let mut t = Vec::with_capacity(THUMB_W * THUMB_H);
            for y in 0..THUMB_H {
                for x in 0..THUMB_W {
                    t.push(px(ox + x, oy + y));
                }
            }
            t
        })
        .collect()
}

/// Palette and set name from the tail of section 0.
fn file_header(s: &[u8]) -> Option<([Rgb; 16], String)> {
    let tail = s.get(s.len().checked_sub(142)?..)?;
    let mut pal = [Rgb(0, 0, 0); 16];
    for (i, c) in pal.iter_mut().enumerate() {
        let v = |k: usize| (be16(tail, 2 + (i * 3 + k) * 2).min(15) * 17) as u8;
        *c = Rgb(v(0), v(1), v(2));
    }
    let name = &tail[110..];
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    Some((pal, crate::formats::latin1(&name[..end]).trim().to_string()))
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
            set_name: String::new(),
            thumbnail: None,
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

    /// Loads an editable asset: `track.png` (16-colour indexed; its palette
    /// is the track palette) and `track.txt` (gates and height map).
    pub fn from_assets(name: &str, png: &[u8], txt: &str) -> Result<Track, String> {
        let img = image::read_indexed(png, &palette::TRACK_DEFAULT)?;
        if (img.w, img.h) != (W, H) {
            return Err(format!(
                "track.png is {}x{}, expected {W}x{H}",
                img.w, img.h
            ));
        }
        if let Some(bad) = img.pixels.iter().find(|&&p| p > 15) {
            return Err(format!(
                "track.png uses palette index {bad}; tracks may only use 0-15"
            ));
        }
        let mut pal = palette::TRACK_DEFAULT;
        if let Some(own) = &img.palette {
            for (dst, src) in pal.iter_mut().zip(own) {
                *dst = *src;
            }
        }

        let mut gates = Vec::new();
        let mut start = (0.0, 0.0);
        let mut flags = 0;
        let mut set_name = String::new();
        let mut height = Vec::with_capacity(HM_W * HM_H);
        let mut in_height = false;
        for (n, line) in txt.lines().enumerate() {
            let err = |m: &str| format!("track.txt line {}: {m}", n + 1);
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or("");
            let nums = |w: std::str::SplitWhitespace| -> Result<Vec<f32>, String> {
                w.map(|x| {
                    x.parse::<f32>()
                        .map_err(|_| err(&format!("`{x}` is not a number")))
                })
                .collect()
            };
            if in_height {
                for x in line.split_whitespace() {
                    height.push(
                        x.parse::<u16>()
                            .map_err(|_| err(&format!("bad height `{x}`")))?,
                    );
                }
                continue;
            }
            match first {
                "gate" => {
                    let v = nums(words)?;
                    if v.len() != 4 {
                        return Err(err("gate needs x1 y1 x2 y2"));
                    }
                    gates.push(Gate {
                        a: (v[0], v[1]),
                        b: (v[2], v[3]),
                    });
                }
                "start" => {
                    let v = nums(words)?;
                    if v.len() != 2 {
                        return Err(err("start needs x y"));
                    }
                    start = (v[0], v[1]);
                }
                "flags" => {
                    flags = words
                        .next()
                        .and_then(|x| x.parse().ok())
                        .ok_or_else(|| err("bad flags"))?
                }
                "height" => in_height = true,
                "set" => set_name = line[3..].trim().to_string(),
                other => return Err(err(&format!("unknown entry `{other}`"))),
            }
        }
        if gates.len() < 2 {
            return Err("track.txt needs at least two gates".into());
        }
        if height.len() != HM_W * HM_H {
            return Err(format!(
                "track.txt height map has {} values, expected {}",
                height.len(),
                HM_W * HM_H
            ));
        }
        Ok(Track {
            name: name.to_string(),
            pixels: img.pixels,
            palette: pal,
            set_name,
            thumbnail: None,
            height,
            gates,
            start,
            flags,
        })
    }

    /// The thumbnail as an indexed PNG in the track's palette.
    pub fn thumb_png(&self) -> Option<Result<Vec<u8>, String>> {
        self.thumbnail
            .as_ref()
            .map(|t| image::write_indexed(THUMB_W, THUMB_H, &self.palette, false, t))
    }

    pub fn set_thumb_png(&mut self, png: &[u8]) -> Result<(), String> {
        let img = image::read_indexed(png, &self.palette)?;
        if (img.w, img.h) != (THUMB_W, THUMB_H) || img.pixels.iter().any(|&p| p > 15) {
            return Err(format!(
                "thumb.png must be {THUMB_W}x{THUMB_H} using colours 0-15"
            ));
        }
        self.thumbnail = Some(img.pixels);
        Ok(())
    }

    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        image::write_indexed(W, H, &self.palette, false, &self.pixels)
    }

    pub fn to_text(&self) -> String {
        let mut t = format!(
            "# Skids track {}. Lines starting with # are comments.\n\
             #\n\
             # The picture is track.png: 1024x512, 16 colours. Its palette is the\n\
             # track's palette. Colours 8-15 are road (full grip), 0-7 are scenery.\n\
             #\n\
             # gate x1 y1 x2 y2: lap gates in race order, in world units. Gate 0 is\n\
             # the start/finish line; the grid forms up behind it. World to picture:\n\
             #   picture x = wx - wy + 512, picture y = (wx + wy) * 31/64 - 252\n",
            self.name
        );
        for g in &self.gates {
            t.push_str(&format!("gate {} {} {} {}\n", g.a.0, g.a.1, g.b.0, g.b.1));
        }
        t.push_str(&format!(
            "\n# Name of the set of tracks this one belongs to, shown in the menu.\nset {}\n\n\
             # As stored in the original data; not used by the game.\nstart {} {}\nflags {}\n\n\
             # height: 64 rows of 64 values, ground height x 256. Each value covers\n\
             # 16x8 pixels of the picture.\nheight\n",
            self.set_name, self.start.0, self.start.1, self.flags
        ));
        for row in self.height.chunks(HM_W) {
            let line: Vec<String> = row.iter().map(|v| v.to_string()).collect();
            t.push_str(&line.join(" "));
            t.push('\n');
        }
        t
    }

    pub fn rgb(&self) -> Vec<u32> {
        let pal: Vec<u32> = self.palette.iter().map(|c| c.to_u32()).collect();
        self.pixels.iter().map(|&i| pal[i as usize]).collect()
    }
}
