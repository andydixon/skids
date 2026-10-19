//! Locates and decodes the original game data.
//!
//! The game's data lives in `assets/` as editable files: per-track PNG and
//! text, per-vehicle sprite sheet and settings (see `track.rs` and
//! `vehicle.rs`). The default build embeds that folder (see `build.rs`), so
//! nothing is needed at run time. The original Amiga data is still read as
//! ADF images, LHA archives, or `track/` and `vehicles/` files; `skids
//! extract` converts it to assets. When both exist, assets win, so edits are
//! never shadowed by the disks they came from.

pub mod embedded;
pub mod image;
pub mod palette;
pub mod track;
pub mod vehicle;

use crate::formats::iff::{self, Picture, Sound};
use crate::formats::{adf::Adf, crm, lha, med};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub use track::{Track, TrackFile};
pub use vehicle::Vehicle;

/// All game files found, keyed by `dir/name` in lower case
/// (e.g. `track/track1`, `vehicles/f1.aga`).
#[derive(Default)]
pub struct FileSet {
    pub files: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
}

impl FileSet {
    pub fn embedded() -> FileSet {
        let mut fs_ = FileSet::default();
        for (path, data) in embedded::files() {
            fs_.add(&path, data.to_vec());
        }
        fs_
    }

    pub fn scan(&mut self, root: &Path) {
        self.scan_dir(root, 0);
    }

    fn scan_dir(&mut self, dir: &Path, depth: usize) {
        let Ok(rd) = fs::read_dir(dir) else {
            self.warnings
                .push(format!("cannot read directory {}", dir.display()));
            return;
        };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if depth < 4 {
                    self.scan_dir(&path, depth + 1);
                }
            } else if let Ok(data) = fs::read(&path) {
                let name = path.to_string_lossy().to_string();
                self.add(&name, data);
            }
        }
    }

    /// Adds a file, unpacking ADF images and LHA archives recursively.
    pub fn add(&mut self, path: &str, data: Vec<u8>) {
        let lower = path.to_lowercase();
        if lower.ends_with(".adf") {
            match Adf::open(data).and_then(|a| a.files()) {
                Ok(files) => {
                    for f in files {
                        self.add(&format!("{path}/{}", f.path), f.data);
                    }
                }
                Err(e) => self.warnings.push(format!("{path}: {e}")),
            }
        } else if lower.ends_with(".lha") || lower.ends_with(".lzh") {
            match lha::parse(&data) {
                Ok(entries) => {
                    for e in entries {
                        match e.data {
                            Ok(d) => self.add(&e.path, d),
                            Err(why) => self.warnings.push(format!("{path}: {}: {why}", e.path)),
                        }
                    }
                }
                Err(e) => self.warnings.push(format!("{path}: {e}")),
            }
        } else if let Some(key) = logical_key(path) {
            self.files.insert(key, data);
        }
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.files.get(key).map(|v| v.as_slice())
    }
}

/// Reduces a path to the key the game looks files up by: `dir/name` for
/// the original data and palettes, `tracks/<name>/<file>` for track assets.
fn logical_key(path: &str) -> Option<String> {
    let parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect();
    let n = parts.len();
    if n >= 3 && parts[n - 3] == "tracks" {
        return Some(format!("tracks/{}/{}", parts[n - 2], parts[n - 1]));
    }
    if n < 2 {
        return None;
    }
    let (dir, name) = (&parts[n - 2], &parts[n - 1]);
    matches!(
        dir.as_str(),
        "track" | "vehicles" | "palettes" | "sfx" | "gfx" | "sounds" | "music"
    )
    .then(|| format!("{dir}/{name}"))
}

/// Unpacks CrunchMania data, passing plain data through untouched.
pub fn unpack(d: &[u8]) -> Result<Vec<u8>, crm::CrmError> {
    if crm::is_crunched(d) {
        crm::decrunch(d)
    } else {
        Ok(d.to_vec())
    }
}

pub enum TrackSource {
    /// `tracks/<name>/` asset files.
    Asset { png: String, txt: String },
    /// A section of an original `TRACKn` file.
    Packed { file: usize, section: usize },
}

pub struct TrackEntry {
    pub name: String,
    pub source: TrackSource,
}

pub struct GameData {
    pub tracks: Vec<TrackEntry>,
    pub track_files: Vec<TrackFile>,
    pub vehicles: Vec<Vehicle>,
    /// Sound effects by lower-case name (`motor3`, `screech2`, ...).
    pub sounds: BTreeMap<String, Sound>,
    /// The tune as an OctaMED module, and the name it is stored under.
    pub music: Option<(String, Vec<u8>)>,
    /// Pictures by lower-case name (`skidtoon`, ...).
    pub pictures: BTreeMap<String, Picture>,
    pub files: FileSet,
    pub warnings: Vec<String>,
}

impl GameData {
    /// Loads the built-in data, overlaid with whatever is under `dir`.
    pub fn load(dir: Option<&Path>) -> Result<GameData, String> {
        Self::load_from(dir, true)
    }

    /// As `load`, optionally without the built-in data (used when
    /// converting original disks, so the old assets do not shadow them).
    pub fn load_from(dir: Option<&Path>, built_in: bool) -> Result<GameData, String> {
        let mut files = if built_in {
            FileSet::embedded()
        } else {
            FileSet::default()
        };
        if let Some(dir) = dir {
            files.scan(dir);
        }
        let mut warnings = files.warnings.clone();

        let mut tracks = Vec::new();
        for key in files.files.keys() {
            if let Some(rest) = key.strip_prefix("tracks/") {
                if let Some(name) = rest.strip_suffix("/track.png") {
                    let txt = format!("tracks/{name}/track.txt");
                    if files.files.contains_key(&txt) {
                        tracks.push(TrackEntry {
                            name: name.to_uppercase(),
                            source: TrackSource::Asset {
                                png: key.clone(),
                                txt,
                            },
                        });
                    } else {
                        warnings.push(format!("{key}: no track.txt next to it, skipped"));
                    }
                }
            }
        }

        let mut track_files = Vec::new();
        for (key, data) in &files.files {
            if let Some(name) = key.strip_prefix("track/") {
                match TrackFile::parse(name, data) {
                    Ok(t) => track_files.push(t),
                    Err(e) => warnings.push(format!("{key}: {e}")),
                }
            }
        }
        track_files.sort_by_key(|t| (t.number, t.name.clone()));
        for (file, tf) in track_files.iter().enumerate() {
            for section in 0..tf.sections.len() {
                let name = tf.track_name(section);
                if !tracks.iter().any(|t| t.name == name) {
                    tracks.push(TrackEntry {
                        name,
                        source: TrackSource::Packed { file, section },
                    });
                }
            }
        }
        tracks.sort_by_key(|t| natural_key(&t.name));

        let mut vehicles: Vec<Vehicle> = Vec::new();
        for (key, data) in &files.files {
            let Some(id) = key
                .strip_prefix("vehicles/")
                .and_then(|n| n.strip_suffix(".png"))
                .filter(|n| !n.ends_with("-menu"))
            else {
                continue;
            };
            let txt = files
                .get(&format!("vehicles/{id}.txt"))
                .map(String::from_utf8_lossy);
            match Vehicle::from_assets(id, data, txt.as_deref()) {
                Ok(mut v) => {
                    if let Some(png) = files.get(&format!("vehicles/{id}-menu.png")) {
                        if let Err(e) = v.set_menu_png(png) {
                            warnings.push(format!("vehicles/{id}-menu.png: {e}"));
                        }
                    }
                    vehicles.push(v)
                }
                Err(e) => warnings.push(format!("{key}: {e}")),
            }
        }
        for (key, data) in &files.files {
            let Some(name) = key
                .strip_prefix("vehicles/")
                .filter(|n| n.ends_with(".aga"))
            else {
                continue;
            };
            if vehicles.iter().any(|v| format!("{}.aga", v.id) == name) {
                continue;
            }
            match Vehicle::parse(name, data) {
                Ok(Some(mut v)) => {
                    if let Some(set) = files.get(&format!("vehicles/{}.set", v.id)) {
                        if let Err(e) = v.apply_set(set) {
                            warnings.push(format!("vehicles/{}.set: {e}", v.id));
                        }
                    }
                    vehicles.push(v)
                }
                Ok(None) => warnings.push(format!("{key}: sprite data is blank, skipped")),
                Err(e) => warnings.push(format!("{key}: {e}")),
            }
        }
        vehicles.sort_by_key(|v| (vehicle::display_order(&v.id), v.id.clone()));

        if tracks.is_empty() || vehicles.is_empty() {
            return Err(format!(
                "no usable game data (found {} tracks, {} vehicles).\n\
                 This build has no data built in: point --data at the assets folder or \
                 the Super Skidmarks ADF images, or rebuild with assets/ in place.{}",
                tracks.len(),
                vehicles.len(),
                if warnings.is_empty() {
                    String::new()
                } else {
                    format!("\n  {}", warnings.join("\n  "))
                }
            ));
        }
        let (sounds, music, pictures) = load_media(&files, &mut warnings);
        Ok(GameData {
            tracks,
            track_files,
            vehicles,
            sounds,
            music,
            pictures,
            files,
            warnings,
        })
    }

    pub fn load_track(&self, i: usize) -> Result<Track, String> {
        let entry = &self.tracks[i];
        let mut t = match &entry.source {
            TrackSource::Asset { png, txt } => {
                let txt = String::from_utf8_lossy(self.files.get(txt).unwrap_or_default());
                Track::from_assets(&entry.name, self.files.get(png).unwrap_or_default(), &txt)
                    .map_err(|e| format!("{}: {e}", entry.name))?
            }
            TrackSource::Packed { file, section } => self.track_files[*file].decode(*section)?,
        };
        if let Some(p) = palette::override_for(&self.files, &entry.name) {
            t.palette = p;
        }
        Ok(t)
    }
}

type Media = (
    BTreeMap<String, Sound>,
    Option<(String, Vec<u8>)>,
    BTreeMap<String, Picture>,
);

/// Sounds, music and pictures. Assets (`sounds/*.wav`, `music/*.med`,
/// `gfx/*.png`) win over the originals (`sfx/` and `gfx/` IFF files, and the
/// CrunchMania-packed tune).
fn load_media(files: &FileSet, warnings: &mut Vec<String>) -> Media {
    let stem = |key: &str, dir: &str, ext: &str| -> Option<String> {
        let name = key.strip_prefix(dir)?.strip_prefix('/')?;
        name.strip_suffix(ext).map(str::to_string)
    };
    let mut sounds = BTreeMap::new();
    // Lowest priority first; later inserts replace earlier ones.
    for dir in ["gfx", "sfx"] {
        for (key, data) in &files.files {
            if let Some(name) = stem(key, dir, ".iff") {
                if iff::is_8svx(data) {
                    match iff::read_8svx(data) {
                        Ok(s) => {
                            sounds.insert(name, s);
                        }
                        Err(e) => warnings.push(format!("{key}: {e}")),
                    }
                }
            }
        }
    }
    for (key, data) in &files.files {
        if let Some(name) = stem(key, "sounds", ".wav") {
            match iff::wav_to_sound(data) {
                Ok(s) => {
                    sounds.insert(name, s);
                }
                Err(e) => warnings.push(format!("{key}: {e}")),
            }
        }
    }

    let mut music = None;
    for (key, data) in &files.files {
        if let Some(name) = stem(key, "sfx", ".crn") {
            match unpack(data) {
                Ok(d) if med::is_med(&d) => music = Some((name, d)),
                Ok(_) => warnings.push(format!("{key}: not an OctaMED module")),
                Err(e) => warnings.push(format!("{key}: {e}")),
            }
        }
    }
    if let Some((key, data)) = files
        .files
        .iter()
        .find(|(k, _)| k.starts_with("music/") && k.ends_with(".med"))
    {
        music = Some((stem(key, "music", ".med").unwrap_or_default(), data.clone()));
    }

    let mut pictures = BTreeMap::new();
    for (key, data) in &files.files {
        if let Some(name) = stem(key, "gfx", ".iff") {
            if data.get(8..12) == Some(b"ILBM") {
                match iff::read_ilbm(data) {
                    Ok(p) => {
                        pictures.insert(name, p);
                    }
                    Err(e) => warnings.push(format!("{key}: {e}")),
                }
            }
        }
    }
    for (key, data) in &files.files {
        if let Some(name) = stem(key, "gfx", ".png") {
            match image::read_indexed(data, &palette::TRACK_DEFAULT) {
                Ok(img) => {
                    let palette = img
                        .palette
                        .unwrap_or_default()
                        .iter()
                        .map(|c| (c.0, c.1, c.2))
                        .collect();
                    pictures.insert(
                        name,
                        Picture {
                            w: img.w,
                            h: img.h,
                            pixels: img.pixels,
                            palette,
                        },
                    );
                }
                Err(e) => warnings.push(format!("{key}: {e}")),
            }
        }
    }
    (sounds, music, pictures)
}

/// Sort key that orders TRACK2-10 after TRACK2-9.
fn natural_key(name: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut num = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                num.clear();
            }
            text.push(c);
        }
    }
    out.push((text, num.parse().unwrap_or(0)));
    out
}
