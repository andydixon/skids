//! Vehicle sprite banks (`vehicles/*.aga`).
//!
//! 409600 bytes once decrunched: 25 tilt blocks x 32 headings x 512 bytes.
//! Each frame is a 64x32 AGA sprite with two bitplanes stored per line
//! (8 bytes plane 0, 8 bytes plane 1), giving three colours plus transparent.
//!
//! Tilt block = roll * 5 + pitch, with 2 meaning level on both axes.
//! Heading frame 12 faces screen-right and frames advance clockwise in
//! 11.25 degree steps.

use super::unpack;

pub const SPR_W: usize = 64;
pub const SPR_H: usize = 32;
pub const HEADINGS: usize = 32;
pub const TILTS: usize = 25;
pub const LEVEL: usize = 12;
const FRAME_BYTES: usize = 512;
const BANK_BYTES: usize = TILTS * HEADINGS * FRAME_BYTES;

pub struct Vehicle {
    pub id: String,
    pub name: String,
    /// Palette indices 0..3, TILTS * HEADINGS frames of SPR_W x SPR_H.
    pub frames: Vec<u8>,
}

impl Vehicle {
    /// Returns `Ok(None)` for banks that are present but blank.
    pub fn parse(file: &str, d: &[u8]) -> Result<Option<Vehicle>, String> {
        let raw = unpack(d).map_err(|e| e.to_string())?;
        if raw.len() < BANK_BYTES {
            return Err(format!("{} bytes, expected {BANK_BYTES}", raw.len()));
        }
        if raw.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let mut frames = vec![0u8; TILTS * HEADINGS * SPR_W * SPR_H];
        for (f, src) in raw[..BANK_BYTES].chunks_exact(FRAME_BYTES).enumerate() {
            let dst = &mut frames[f * SPR_W * SPR_H..(f + 1) * SPR_W * SPR_H];
            for y in 0..SPR_H {
                let line = &src[y * 16..y * 16 + 16];
                for x in 0..SPR_W {
                    let mask = 0x80 >> (x & 7);
                    let p0 = (line[x >> 3] & mask != 0) as u8;
                    let p1 = (line[8 + (x >> 3)] & mask != 0) as u8;
                    dst[y * SPR_W + x] = p0 | (p1 << 1);
                }
            }
        }
        let id = file.trim_end_matches(".aga").to_lowercase();
        Ok(Some(Vehicle {
            name: display_name(&id),
            id,
            frames,
        }))
    }

    pub fn frame(&self, tilt: usize, heading: usize) -> &[u8] {
        let f = tilt.min(TILTS - 1) * HEADINGS + heading % HEADINGS;
        &self.frames[f * SPR_W * SPR_H..(f + 1) * SPR_W * SPR_H]
    }
}

/// Heading frame for a world-space angle (radians, 0 = +wx). World +wx runs
/// down-right on screen, so screen-right (frame 12) is world angle -45 degrees.
pub fn heading_frame(angle: f32) -> usize {
    let steps = (angle + std::f32::consts::FRAC_PI_4) / (std::f32::consts::TAU / HEADINGS as f32);
    (12 + steps.round() as i32).rem_euclid(HEADINGS as i32) as usize
}

pub fn tilt_block(pitch: i32, roll: i32) -> usize {
    ((roll.clamp(-2, 2) + 2) * 5 + (pitch.clamp(-2, 2) + 2)) as usize
}

fn display_name(id: &str) -> String {
    match id {
        "vw" => "BEETLE".into(),
        "f1" => "FORMULA 1".into(),
        other => other.to_uppercase(),
    }
}

pub fn display_order(id: &str) -> usize {
    const ORDER: [&str; 8] = [
        "mini", "vw", "porsche", "f1", "midget", "truck", "police", "cow",
    ];
    ORDER.iter().position(|&o| o == id).unwrap_or(ORDER.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_frames() {
        use std::f32::consts::{FRAC_PI_4, PI};
        assert_eq!(heading_frame(-FRAC_PI_4), 12);
        assert_eq!(heading_frame(FRAC_PI_4), 20);
        assert_eq!(heading_frame(PI - FRAC_PI_4), 28);
        assert_eq!(heading_frame(-PI + FRAC_PI_4), 4);
    }

    #[test]
    fn level_block() {
        assert_eq!(tilt_block(0, 0), LEVEL);
    }
}
