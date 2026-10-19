//! Locates and decodes the original game data.
//!
//! The default build embeds the data from the ADF images it was built
//! alongside (see `build.rs`), so no disks are needed at run time. Extra or
//! replacement data can still be supplied as ADF images, the LHA archives
//! from them, or an extracted tree with `track/`, `vehicles/` and `palettes/`.
//! Files on disk override the built-in ones.

pub mod embedded;
pub mod palette;
pub mod track;
pub mod vehicle;

use crate::formats::{adf::Adf, crm, lha};
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
                            Some(d) => self.add(&e.path, d),
                            None => self.warnings.push(format!(
                                "{path}: {} uses unsupported method {}",
                                e.path, e.method
                            )),
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

/// Reduces a path to `dir/name` if `dir` is one the game uses.
fn logical_key(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let dir = parts[parts.len() - 2].to_lowercase();
    let name = parts[parts.len() - 1].to_lowercase();
    matches!(dir.as_str(), "track" | "vehicles" | "palettes").then(|| format!("{dir}/{name}"))
}

/// Unpacks CrunchMania data, passing plain data through untouched.
pub fn unpack(d: &[u8]) -> Result<Vec<u8>, crm::CrmError> {
    if crm::is_crunched(d) {
        crm::decrunch(d)
    } else {
        Ok(d.to_vec())
    }
}

pub struct GameData {
    pub track_files: Vec<TrackFile>,
    pub vehicles: Vec<Vehicle>,
    pub files: FileSet,
    pub warnings: Vec<String>,
}

impl GameData {
    /// Loads the built-in data, overlaid with whatever is under `dir`.
    pub fn load(dir: Option<&Path>) -> Result<GameData, String> {
        let mut files = FileSet::embedded();
        if let Some(dir) = dir {
            files.scan(dir);
        }
        let mut warnings = files.warnings.clone();

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

        let mut vehicles = Vec::new();
        for (key, data) in &files.files {
            if let Some(name) = key.strip_prefix("vehicles/") {
                match Vehicle::parse(name, data) {
                    Ok(Some(v)) => vehicles.push(v),
                    Ok(None) => warnings.push(format!("{key}: sprite data is blank, skipped")),
                    Err(e) => warnings.push(format!("{key}: {e}")),
                }
            }
        }
        vehicles.sort_by_key(|v| vehicle::display_order(&v.id));

        if track_files.is_empty() || vehicles.is_empty() {
            return Err(format!(
                "no usable game data (found {} track files, {} vehicles).\n\
                 This build has no data built in: point --data at a folder containing the \
                 Super Skidmarks ADF images, or rebuild with the images next to Cargo.toml.{}",
                track_files.len(),
                vehicles.len(),
                if warnings.is_empty() {
                    String::new()
                } else {
                    format!("\n  {}", warnings.join("\n  "))
                }
            ));
        }
        Ok(GameData {
            track_files,
            vehicles,
            files,
            warnings,
        })
    }

    /// Flattened (file, section) list of every playable track.
    pub fn track_list(&self) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (fi, tf) in self.track_files.iter().enumerate() {
            for si in 0..tf.sections.len() {
                v.push((fi, si));
            }
        }
        v
    }

    pub fn load_track(&self, file: usize, section: usize) -> Result<Track, String> {
        let tf = &self.track_files[file];
        let mut t = tf.decode(section)?;
        if let Some(p) = palette::override_for(&self.files, &tf.name, section) {
            t.palette = p;
        }
        Ok(t)
    }
}
