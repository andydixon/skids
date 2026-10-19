//! Vehicles: sprite banks and handling.
//!
//! Editable assets (`assets/vehicles/<id>.png` + `<id>.txt`): the PNG is a
//! sheet of 32 headings across by 25 tilt blocks down, each cell 64x32, with
//! palette indices 0 (transparent), 1 (tyres/shadow), 2 (trim), 3 (body).
//! Its colours are only a preview; cars are recoloured per player. The text
//! file holds the display name and handling.
//!
//! Original data (`vehicles/*.aga`):
//! 409600 bytes once decrunched: 25 tilt blocks x 32 headings x 512 bytes.
//! Each frame is a 64x32 AGA sprite with two bitplanes stored per line
//! (8 bytes plane 0, 8 bytes plane 1), giving three colours plus transparent.
//!
//! Tilt block = roll * 5 + pitch, with 2 meaning level on both axes.
//! Heading frame 12 faces screen-right and frames advance clockwise in
//! 11.25 degree steps.

use super::image;
use super::palette::{self, Rgb};
use super::unpack;

pub const SPR_W: usize = 64;
pub const SPR_H: usize = 32;
pub const HEADINGS: usize = 32;
pub const TILTS: usize = 25;
pub const LEVEL: usize = 12;
const FRAME_BYTES: usize = 512;
const BANK_BYTES: usize = TILTS * HEADINGS * FRAME_BYTES;

pub const SHEET_W: usize = SPR_W * HEADINGS;
pub const SHEET_H: usize = SPR_H * TILTS;

/// Handling. The retail executable is not available, so these values are a
/// reconstruction; edit them in the vehicle's `.txt` asset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    /// Top speed on tarmac, world pixels per second.
    pub top: f32,
    pub accel: f32,
    pub brake: f32,
    /// Lateral grip: how quickly sideways motion is killed (1/s).
    pub grip: f32,
    /// Steering rate at speed (rad/s).
    pub turn: f32,
    pub mass: f32,
    /// Speed and grip multiplier off the road.
    pub offroad: f32,
}

pub const BASE_SPEC: Spec = Spec {
    top: 200.0,
    accel: 170.0,
    brake: 320.0,
    grip: 5.0,
    turn: 3.6,
    mass: 1.0,
    offroad: 0.55,
};

/// Starting values used when importing the original data.
pub fn default_spec(id: &str) -> Spec {
    let b = BASE_SPEC;
    match id {
        "f1" => Spec {
            top: 240.0,
            accel: 210.0,
            grip: 6.5,
            turn: 3.7,
            mass: 0.9,
            offroad: 0.45,
            ..b
        },
        "porsche" => Spec {
            top: 225.0,
            accel: 190.0,
            grip: 5.2,
            turn: 3.6,
            ..b
        },
        "mini" => Spec {
            top: 190.0,
            accel: 200.0,
            grip: 5.8,
            turn: 4.4,
            mass: 0.8,
            ..b
        },
        "vw" => Spec {
            top: 185.0,
            accel: 160.0,
            grip: 4.6,
            turn: 3.6,
            mass: 1.0,
            ..b
        },
        "midget" => Spec {
            top: 205.0,
            accel: 185.0,
            grip: 4.0,
            turn: 4.0,
            mass: 0.85,
            offroad: 0.6,
            ..b
        },
        "truck" => Spec {
            top: 195.0,
            accel: 140.0,
            grip: 4.2,
            turn: 3.0,
            mass: 1.8,
            offroad: 0.75,
            ..b
        },
        "police" => Spec {
            top: 215.0,
            accel: 180.0,
            grip: 5.0,
            turn: 3.5,
            mass: 1.2,
            ..b
        },
        "cow" => Spec {
            top: 170.0,
            accel: 150.0,
            grip: 7.0,
            turn: 4.6,
            mass: 1.4,
            offroad: 0.9,
            ..b
        },
        _ => b,
    }
}

pub struct Vehicle {
    pub id: String,
    pub name: String,
    pub spec: Spec,
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
            spec: default_spec(&id),
            id,
            frames,
        }))
    }

    /// Loads an editable asset: sprite sheet PNG plus optional settings text.
    pub fn from_assets(id: &str, png: &[u8], txt: Option<&str>) -> Result<Vehicle, String> {
        let img = image::read_indexed(png, &preview_palette())?;
        if (img.w, img.h) != (SHEET_W, SHEET_H) {
            return Err(format!(
                "sheet is {}x{}, expected {SHEET_W}x{SHEET_H}",
                img.w, img.h
            ));
        }
        if let Some(bad) = img.pixels.iter().find(|&&p| p > 3) {
            return Err(format!("uses palette index {bad}; cars may only use 0-3"));
        }
        let mut frames = vec![0u8; TILTS * HEADINGS * SPR_W * SPR_H];
        for tilt in 0..TILTS {
            for hd in 0..HEADINGS {
                let f = (tilt * HEADINGS + hd) * SPR_W * SPR_H;
                for y in 0..SPR_H {
                    let src = (tilt * SPR_H + y) * SHEET_W + hd * SPR_W;
                    frames[f + y * SPR_W..f + (y + 1) * SPR_W]
                        .copy_from_slice(&img.pixels[src..src + SPR_W]);
                }
            }
        }
        let mut v = Vehicle {
            id: id.to_string(),
            name: display_name(id),
            spec: default_spec(id),
            frames,
        };
        if let Some(txt) = txt {
            v.apply_settings(txt)?;
        }
        Ok(v)
    }

    pub fn sheet_png(&self) -> Result<Vec<u8>, String> {
        let mut px = vec![0u8; SHEET_W * SHEET_H];
        for tilt in 0..TILTS {
            for hd in 0..HEADINGS {
                let fr = self.frame(tilt, hd);
                for y in 0..SPR_H {
                    let dst = (tilt * SPR_H + y) * SHEET_W + hd * SPR_W;
                    px[dst..dst + SPR_W].copy_from_slice(&fr[y * SPR_W..(y + 1) * SPR_W]);
                }
            }
        }
        image::write_indexed(SHEET_W, SHEET_H, &preview_palette(), true, &px)
    }

    pub fn settings_text(&self) -> String {
        let s = &self.spec;
        format!(
            "# Skids vehicle settings. Lines starting with # are comments.\n\
             # The sprite sheet is {id}.png: 32 headings across, 25 tilts down.\n\
             name {name}\n\
             # top speed on tarmac, world pixels per second\n\
             top {top}\n\
             # acceleration and braking, world pixels per second squared\n\
             accel {accel}\n\
             brake {brake}\n\
             # sideways grip (higher = less sliding)\n\
             grip {grip}\n\
             # steering rate, radians per second\n\
             turn {turn}\n\
             # weight in collisions\n\
             mass {mass}\n\
             # speed and grip multiplier off the road\n\
             offroad {offroad}\n",
            id = self.id,
            name = self.name,
            top = s.top,
            accel = s.accel,
            brake = s.brake,
            grip = s.grip,
            turn = s.turn,
            mass = s.mass,
            offroad = s.offroad
        )
    }

    fn apply_settings(&mut self, txt: &str) -> Result<(), String> {
        for (n, line) in txt.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, val)) = line.split_once(char::is_whitespace) else {
                if line.is_empty() {
                    continue;
                }
                return Err(format!("line {}: expected `key value`", n + 1));
            };
            let val = val.trim();
            if key == "name" {
                self.name = val.to_uppercase();
                continue;
            }
            let num: f32 = val
                .parse()
                .map_err(|_| format!("line {}: `{val}` is not a number", n + 1))?;
            let s = &mut self.spec;
            match key {
                "top" => s.top = num,
                "accel" => s.accel = num,
                "brake" => s.brake = num,
                "grip" => s.grip = num,
                "turn" => s.turn = num,
                "mass" => s.mass = num.max(0.05),
                "offroad" => s.offroad = num,
                _ => return Err(format!("line {}: unknown setting `{key}`", n + 1)),
            }
        }
        Ok(())
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

/// Colours stored in sprite sheets so they preview sensibly in an editor.
fn preview_palette() -> Vec<Rgb> {
    let p = palette::car_palette(0);
    let rgb = |c: u32| Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
    vec![Rgb(0x30, 0x48, 0x30), rgb(p[1]), rgb(p[2]), rgb(p[3])]
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
    fn settings_round_trip() {
        let mut v = Vehicle {
            id: "f1".into(),
            name: "X".into(),
            spec: BASE_SPEC,
            frames: vec![0; TILTS * HEADINGS * SPR_W * SPR_H],
        };
        v.spec.grip = 6.25;
        v.name = "FORMULA 1".into();
        let txt = v.settings_text();
        let mut w = Vehicle {
            id: "f1".into(),
            name: String::new(),
            spec: BASE_SPEC,
            frames: Vec::new(),
        };
        w.apply_settings(&txt).unwrap();
        assert_eq!(w.spec, v.spec);
        assert_eq!(w.name, "FORMULA 1");
        assert!(w.apply_settings("grip fast").is_err());
    }

    #[test]
    fn level_block() {
        assert_eq!(tilt_block(0, 0), LEVEL);
    }
}
