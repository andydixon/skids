//! Colour palettes.
//!
//! Each original track file carries the 16-colour palette its six tracks
//! share (see `track.rs`). `TRACK_DEFAULT` is only a fallback for track
//! pictures that arrive without one: indices 0-7 scenery, 8-15 road.
//!
//! Each track asset's PNG carries its own palette, so the usual way to
//! recolour a track is to edit that. Palettes can also be dropped in as text
//! files (16 lines of `RRGGBB`, `#` comments allowed), which win over both:
//!   palettes/track1-3.pal   one track
//!   palettes/track1.pal     all six tracks in TRACK1
//!   palettes/default.pal    every track

use super::FileSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn to_u32(self) -> u32 {
        ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    pub fn scale(self, f: f32) -> Rgb {
        let s = |c: u8| ((c as f32 * f).round().clamp(0.0, 255.0)) as u8;
        Rgb(s(self.0), s(self.1), s(self.2))
    }

    pub fn mix(self, o: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }
}

pub const TRACK_DEFAULT: [Rgb; 16] = [
    // scenery: deep shadow through sunlit grass
    Rgb(0x0c, 0x1a, 0x0e),
    Rgb(0x18, 0x2c, 0x14),
    Rgb(0x24, 0x40, 0x1a),
    Rgb(0x34, 0x56, 0x20),
    Rgb(0x48, 0x6c, 0x28),
    Rgb(0x60, 0x84, 0x34),
    Rgb(0x84, 0x9c, 0x4c),
    Rgb(0xb0, 0xb8, 0x7c),
    // driving surface: tarmac, kerbs and markings
    Rgb(0x3a, 0x36, 0x34),
    Rgb(0x50, 0x4c, 0x48),
    Rgb(0x66, 0x62, 0x5c),
    Rgb(0x7c, 0x78, 0x70),
    Rgb(0x94, 0x90, 0x86),
    Rgb(0xae, 0xaa, 0x9e),
    Rgb(0xcc, 0xc8, 0xbc),
    Rgb(0xf0, 0xee, 0xe4),
];

/// Per-player body colours for the three-colour vehicle sprites
/// (index 1 = tyres and shadow, 2 = trim, 3 = bodywork).
pub const TEAM_COLOURS: [Rgb; 8] = [
    Rgb(0xe0, 0x30, 0x28),
    Rgb(0x30, 0x60, 0xe8),
    Rgb(0xf0, 0xc8, 0x20),
    Rgb(0x30, 0xb0, 0x48),
    Rgb(0xe0, 0x60, 0xd0),
    Rgb(0x20, 0xc0, 0xc8),
    Rgb(0xf0, 0x80, 0x20),
    Rgb(0xe8, 0xe8, 0xe8),
];

pub fn car_palette(team: usize) -> [u32; 4] {
    let c = TEAM_COLOURS[team % TEAM_COLOURS.len()];
    [
        0,
        Rgb(0x10, 0x10, 0x14).to_u32(),
        c.scale(0.7).to_u32(),
        c.mix(Rgb(0xff, 0xff, 0xff), 0.35).to_u32(),
    ]
}

pub fn parse(text: &str) -> Option<[Rgb; 16]> {
    let mut out = [Rgb(0, 0, 0); 16];
    let mut n = 0;
    for line in text.lines() {
        let line = line
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches("0x");
        if line.is_empty() {
            continue;
        }
        if n == 16 || line.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(line, 16).ok()?;
        out[n] = Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        n += 1;
    }
    (n == 16).then_some(out)
}

/// A palette file for a track named like `TRACK1-3`, most specific first.
pub fn override_for(files: &FileSet, track: &str) -> Option<[Rgb; 16]> {
    let name = track.to_lowercase();
    let group = name.split('-').next().unwrap_or(&name).to_string();
    [
        format!("palettes/{name}.pal"),
        format!("palettes/{group}.pal"),
        "palettes/default.pal".into(),
    ]
    .iter()
    .find_map(|k| {
        files
            .get(k)
            .and_then(|d| parse(&String::from_utf8_lossy(d)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_palette_file() {
        let mut text = String::from("# test\n");
        for i in 0..16 {
            text.push_str(&format!("{:02x}{:02x}{:02x}\n", i, i * 2, i * 3));
        }
        let p = parse(&text).unwrap();
        assert_eq!(p[15], Rgb(15, 30, 45));
        assert!(parse("ffffff\n").is_none());
    }
}
