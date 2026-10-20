//! Vehicles: sprite banks and handling.
//!
//! Editable assets (`assets/vehicles/<id>.png` + `<id>.txt`): the PNG is a
//! sheet of 32 headings across by 25 tilt blocks down, each cell 64x32, with
//! palette indices 0 (transparent), 1 (tyres/shadow), 2 (trim), 3 (body).
//! Its colours are only a preview; cars are recoloured per player. The text
//! file holds the display name and handling.
//!
//! Settings (`vehicles/*.set`, disk 1): car name and driver name, each
//! ending in a newline, then 544 bytes: 16 BE16 handling values (see
//! `SetValues`) and a 64x32 two-colour-plane picture for the menu, plane 0
//! then plane 1.
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

/// The 16 handling values of a `.set` file. Their meaning, from the
/// executable's set-up code (word k is `set[k]`):
///
/// | Word | Use |
/// |------|-----|
/// | 0, 1, 5 | computer drivers' drag, grip and power |
/// | 2 | steering rate |
/// | 3 | read into the car but never used |
/// | 4 | gravity |
/// | 6 | tow hitch distance behind the car |
/// | 8, 9, 10 | human drivers' drag, grip and power |
///
/// Words 7 and 11-15 aren't read.
pub type SetValues = [u16; 16];

/// The Camaro's values, for vehicles that come without any.
pub const DEFAULT_SET: SetValues = [54, 35, 32, 8, 7, 22, 22, 16, 49, 14, 53, 0, 0, 0, 0, 0];

/// One driver profile. The original's per-frame update (in its own units,
/// where one world unit is a height-map cell) is:
///
/// ```text
/// target = (speed + throttle * power) * drag
/// velocity += grip * (heading * target - velocity)
/// speed += (target - speed) / 4
/// ```
///
/// so top speed is `power * drag / (1 - drag)` per frame, and `grip` is
/// the fraction of the gap between where the car points and where it is
/// going that closes each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handling {
    pub drag: f32,
    pub grip: f32,
    pub power: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub player: Handling,
    pub computer: Handling,
    /// Steering at full lock, in 64ths of a turn per frame.
    pub turn: f32,
    /// Pixels per frame per frame.
    pub gravity: f32,
    /// Tow hitch distance behind the car's centre, world units.
    pub hitch: f32,
    /// Weight in collisions between cars. Not from the original data.
    pub mass: f32,
}

impl Spec {
    /// Decodes the values as the executable does (fixed-point shifts).
    pub fn from_set(v: &SetValues, mass: f32) -> Spec {
        let f = |k: usize, shift: u32| v[k] as i16 as f32 / (1u32 << shift) as f32;
        Spec {
            player: Handling {
                drag: f(8, 6),
                grip: f(9, 9),
                power: f(10, 9),
            },
            computer: Handling {
                drag: f(0, 6),
                grip: f(1, 9),
                power: f(5, 9),
            },
            turn: f(2, 4),
            gravity: f(4, 3),
            hitch: f(6, 5) * super::track::CELL,
            mass,
        }
    }
}

/// The original's handling presets, which replace every car's own values
/// (its "CUSTOM" setting). Bytes 0-2 are drag, top speed and grip for
/// human drivers, 3-5 the same for computer drivers, and 6 is steering.
pub const PRESETS: [(&str, [u8; 7]); 6] = [
    ("CLASSIC", [98, 26, 24, 98, 24, 24, 44]),
    ("GRUNTY", [92, 40, 30, 108, 33, 25, 60]),
    ("ZIPPY", [102, 61, 47, 115, 59, 45, 64]),
    ("RIPPY", [100, 80, 60, 110, 50, 60, 72]),
    ("SLIPPERY", [25, 127, 101, 29, 45, 122, 68]),
    ("PEDAL CAR", [117, 23, 17, 104, 17, 10, 30]),
];

impl Handling {
    /// Decodes a preset's three bytes as the executable does: top speed
    /// comes out at `top / 128` world units per frame.
    fn from_preset(drag: u8, top: u8, grip: u8) -> Handling {
        let drag = drag as f32 / 128.0 * 0.98 + 0.01;
        let top = top as f32 / 16.0;
        let power = top * (1.0 / drag - 1.0);
        Handling {
            drag,
            grip: grip as f32 / 256.0 / power / drag / 8.0,
            power: power / 8.0,
        }
    }
}

impl Spec {
    /// The spec with preset `n` (1-based, as in the original's menu; 0
    /// keeps the car's own values).
    pub fn with_preset(self, n: usize) -> Spec {
        let Some((_, b)) = n.checked_sub(1).and_then(|i| PRESETS.get(i)) else {
            return self;
        };
        Spec {
            player: Handling::from_preset(b[0], b[1], b[2]),
            computer: Handling::from_preset(b[3], b[4], b[5]),
            turn: b[6] as f32 / 32.0,
            gravity: 0.5,
            ..self
        }
    }
}

/// Collision weight. The original's car-to-car physics isn't decoded, so
/// these are guesses.
pub fn default_mass(id: &str) -> f32 {
    match id {
        "f1" => 0.9,
        "mini" => 0.8,
        "midget" => 0.85,
        "truck" => 1.8,
        "police" => 1.2,
        "cow" => 1.4,
        _ => 1.0,
    }
}

pub struct Vehicle {
    pub id: String,
    pub name: String,
    pub driver: String,
    /// Towed behind cars rather than driven (the caravan).
    pub trailer: bool,
    /// The 16 handling values from the original `.set` file.
    pub set: SetValues,
    /// 64x32 menu picture (indices 0-3), from the `.set` file.
    pub menu_picture: Option<Vec<u8>>,
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
            driver: String::new(),
            trailer: id == "caravan",
            set: DEFAULT_SET,
            menu_picture: None,
            spec: Spec::from_set(&DEFAULT_SET, default_mass(&id)),
            id,
            frames,
        }))
    }

    /// Applies an original `.set` file.
    pub fn apply_set(&mut self, d: &[u8]) -> Result<(), String> {
        let nl = |from: usize| d[from..].iter().position(|&b| b == b'\n').map(|p| p + from);
        let a = nl(0).ok_or("no name line")?;
        let b = nl(a + 1).ok_or("no driver line")?;
        let body = &d[b + 1..];
        if body.len() < 32 + 512 {
            return Err(format!("{} bytes of settings, expected 544", body.len()));
        }
        let name = crate::formats::latin1(&d[..a]).trim().to_uppercase();
        if !matches!(self.id.as_str(), "f1" | "vw") && !name.is_empty() {
            self.name = name;
        }
        self.driver = crate::formats::latin1(&d[a + 1..b]).trim().to_uppercase();
        for (i, v) in self.set.iter_mut().enumerate() {
            *v = crate::formats::be16(body, i * 2);
        }
        self.spec = Spec::from_set(&self.set, self.spec.mass);
        let pic = &body[32..32 + 512];
        let mut px = vec![0u8; SPR_W * SPR_H];
        for (i, p) in px.iter_mut().enumerate() {
            let (byte, mask) = (i / 8, 0x80 >> (i % 8));
            *p = (pic[byte] & mask != 0) as u8 | (((pic[256 + byte] & mask) != 0) as u8) << 1;
        }
        self.menu_picture = Some(px);
        Ok(())
    }

    pub fn menu_png(&self) -> Option<Result<Vec<u8>, String>> {
        self.menu_picture
            .as_ref()
            .map(|px| image::write_indexed(SPR_W, SPR_H, &preview_palette(), true, px))
    }

    pub fn set_menu_png(&mut self, png: &[u8]) -> Result<(), String> {
        let img = image::read_indexed(png, &preview_palette())?;
        if (img.w, img.h) != (SPR_W, SPR_H) || img.pixels.iter().any(|&p| p > 3) {
            return Err(format!(
                "menu picture must be {SPR_W}x{SPR_H} using colours 0-3"
            ));
        }
        self.menu_picture = Some(img.pixels);
        Ok(())
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
            driver: String::new(),
            trailer: id == "caravan",
            set: DEFAULT_SET,
            menu_picture: None,
            spec: Spec::from_set(&DEFAULT_SET, default_mass(id)),
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
        format!(
            "# Skids vehicle settings. Lines starting with # are comments.\n\
             # The sprite sheet is {id}.png: 32 headings across, 25 tilts down.\n\
             name {name}\n\
             driver {driver}\n\
             # yes = towed behind cars (Caravans option) instead of driven\n\
             trailer {trailer}\n\
             #\n\
             # handling: the 16 values from the original .set file, used as the\n\
             # original game does. Counting from 1:\n\
             #   1 2 6    computer drivers: drag, grip, power\n\
             #   9 10 11  human drivers: drag, grip, power\n\
             #   3 steering, 5 gravity, 7 tow hitch distance; the rest are unused.\n\
             # Top speed = power * drag / (64 - drag) / 32 world units per frame,\n\
             # 50 frames a second (drag below 64). Higher grip, less sliding.\n\
             handling {set}\n\
             # weight when cars collide (not from the original data)\n\
             mass {mass}\n",
            id = self.id,
            name = self.name,
            driver = self.driver,
            trailer = if self.trailer { "yes" } else { "no" },
            set = self
                .set
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            mass = self.spec.mass,
        )
    }

    fn apply_settings(&mut self, txt: &str) -> Result<(), String> {
        for (n, line) in txt.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (key, val) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let val = val.trim();
            let bad = |what: &str| format!("line {}: {what}", n + 1);
            match key {
                "name" => self.name = val.to_uppercase(),
                "driver" => self.driver = val.to_uppercase(),
                "trailer" => self.trailer = matches!(val, "yes" | "true" | "1"),
                // `original` is what older asset files called it.
                "handling" | "original" => {
                    let vals: Vec<u16> = val
                        .split_whitespace()
                        .map(|x| x.parse::<u16>())
                        .collect::<Result<_, _>>()
                        .map_err(|_| bad("handling values must be whole numbers 0-65535"))?;
                    if vals.len() > 16 {
                        return Err(bad("handling has more than 16 values"));
                    }
                    self.set = [0; 16];
                    self.set[..vals.len()].copy_from_slice(&vals);
                }
                "mass" => {
                    self.spec.mass = val
                        .parse::<f32>()
                        .map_err(|_| bad(&format!("`{val}` is not a number")))?
                        .max(0.05)
                }
                // Settings from before the original handling was decoded.
                "top" | "accel" | "brake" | "grip" | "turn" | "offroad" => {}
                _ => return Err(bad(&format!("unknown setting `{key}`"))),
            }
        }
        self.spec = Spec::from_set(&self.set, self.spec.mass);
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
        let blank = |frames| Vehicle {
            id: "f1".into(),
            name: String::new(),
            driver: String::new(),
            trailer: false,
            set: DEFAULT_SET,
            menu_picture: None,
            spec: Spec::from_set(&DEFAULT_SET, 1.0),
            frames,
        };
        let mut v = blank(vec![0; TILTS * HEADINGS * SPR_W * SPR_H]);
        v.driver = "SIMON".into();
        v.set = [60, 71, 30, 0, 6, 16, 24, 12, 58, 42, 29, 0, 0, 0, 0, 0];
        v.spec = Spec::from_set(&v.set, 0.9);
        v.name = "FORMULA 1".into();
        let txt = v.settings_text();
        let mut w = blank(Vec::new());
        w.apply_settings(&txt).unwrap();
        assert_eq!(w.spec, v.spec);
        assert_eq!(w.name, "FORMULA 1");
        assert_eq!(w.driver, "SIMON");
        assert_eq!(w.set, v.set);
        assert!(w.apply_settings("mass heavy").is_err());
        // Older files with reconstructed handling still load.
        assert!(w.apply_settings("original 1 2 3\ntop 200").is_ok());
        assert_eq!(&w.set[..4], &[1, 2, 3, 0]);
    }

    #[test]
    fn decodes_set_values() {
        let s = Spec::from_set(&DEFAULT_SET, 1.0);
        assert_eq!(s.player.drag, 49.0 / 64.0);
        assert_eq!(s.computer.power, 22.0 / 512.0);
        assert_eq!(s.turn, 2.0);
        assert_eq!(s.hitch, 11.0);
        let p = s.with_preset(1);
        let top = p.player.power * p.player.drag / (1.0 - p.player.drag);
        assert!((top - 26.0 / 128.0).abs() < 1e-5);
        assert_eq!(s.with_preset(0), s);
    }

    #[test]
    fn level_block() {
        assert_eq!(tilt_block(0, 0), LEVEL);
    }
}
